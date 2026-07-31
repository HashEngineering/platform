//! General Core L1 payment building.
//!
//! [`CoreWallet::build_signed_payment`] is the first-class "send" primitive:
//! it selects inputs from **one** caller-named funds account, builds and signs
//! a standard payment transaction, and returns the **signed serialized bytes**
//! plus the computed fee and change amount — WITHOUT broadcasting and WITHOUT
//! persisting a debit.
//!
//! ## Funding-domain isolation
//!
//! Selection is confined to a single funding account, defaulting to the unmixed
//! BIP44 account — never a union across accounts. See
//! [`crate::wallet::funding_privacy`] for the invariant, the
//! dashpay/platform#4073 → #4184 history behind it, and the guardrail that
//! enforces it.
//!
//! ## Why build-only / no-broadcast
//!
//! During the dashj→SDK transition the Android app keeps its own transaction
//! bookkeeping (dashj's `maybeCommitTx` drives CrowdNode, memos, and confidence
//! listeners). The app therefore wants the SDK to *build + sign* a payment from
//! the bound wallet and hand back the raw bytes, then commit + broadcast them
//! through dashj itself. Post-transition a separate SDK-broadcast mode will own
//! broadcasting and the debit persistence that goes with it; this primitive is
//! the permanent, generally-useful "give me signed bytes" half of that split.
//!
//! ## Persistence semantics (deliberate)
//!
//! Building does **not** persist a debit and does not write UTXOs, balances, or
//! transaction records back to the wallet. The only in-memory mutation is the
//! key-wallet `ReservationSet` bookkeeping that `set_funding` +
//! `TransactionBuilder::build_signed` perform on the **selected** funding
//! account: the selected inputs are marked *reserved* so a concurrent SDK build
//! does not re-select the same coins. Because selection is confined to one
//! account, every selected input is reserved in the ledger that all funding
//! paths consult for that account — there are no unreserved "secondary-account"
//! inputs (dashpay/platform#4247 review finding, now structurally impossible).
//!
//! That reservation is in-memory only (never
//! serialized) and is released when the spend is later processed back into the
//! wallet by sync, or by the reservation-TTL backstop, or explicitly via
//! [`ManagedCoreFundsAccount::release_reservation`] for an abandoned build. No
//! balance is debited until the transaction actually confirms — exactly what
//! the transition flow needs, since dashj owns commit/broadcast.
//!
//! [`ManagedCoreFundsAccount::release_reservation`]:
//!     key_wallet::managed_account::ManagedCoreFundsAccount::release_reservation

use std::collections::HashMap;

use dashcore::{Address as DashAddress, OutPoint, Transaction};
use key_wallet::bip32::DerivationPath;
use key_wallet::managed_account::managed_account_trait::ManagedAccountTrait;
use key_wallet::managed_account::ManagedCoreFundsAccount;
use key_wallet::signer::Signer;
use key_wallet::wallet::managed_wallet_info::coin_selection::{SelectionError, SelectionStrategy};
use key_wallet::wallet::managed_wallet_info::fee::FeeRate;
use key_wallet::wallet::managed_wallet_info::transaction_builder::{BuilderError, TransactionBuilder};
use key_wallet::wallet::managed_wallet_info::wallet_info_interface::WalletInfoInterface;
use key_wallet::ReservationToken as KeyWalletReservationToken;

use crate::broadcaster::TransactionBroadcaster;
use crate::error::PlatformWalletError;
use crate::wallet::core::transaction::FundingAccountRef;
use crate::wallet::core::CoreWallet;
use crate::wallet::funding_privacy::is_signable_funding_account;

/// key-wallet's default fee rate (duffs per kB). Matches the asset-lock
/// builder's `DEFAULT_FEE_PER_KB` and `FeeRate::normal()`.
const DEFAULT_FEE_PER_KB: u64 = 1000;

/// Consensus cap on any single amount this primitive will accept or aggregate.
const MAX_MONEY: u64 = dashcore::blockdata::constants::MAX_MONEY;

/// Upper bound on the caller-supplied fee rate, in duffs/kB.
///
/// Derived so that even a maximum-size standard transaction cannot produce a
/// fee above [`MAX_MONEY`]: Dash's standard-transaction limit is 100_000 bytes,
/// i.e. 100 kB, and `FeeRate::calculate_fee` computes
/// `sat_per_kb * size_bytes / 1000` — so `MAX_MONEY / 100` also keeps the
/// intermediate `sat_per_kb * size_bytes` product (≤ 2.1e18) inside `u64`.
const MAX_FEE_PER_KB: u64 = MAX_MONEY / 100;

/// The unmixed BIP44 account this primitive is pinned to, in both of its roles:
///
/// * the **default funding account** — where `funding_path: None` selects from;
/// * the **change sink** — key-wallet derives change addresses only for
///   *Standard* accounts, so a payment funded from an explicitly-named
///   non-Standard account (CoinJoin / DashPay-receiving) must route change
///   here. See [`crate::wallet::funding_privacy`].
///
/// Account 0 rather than a caller-chosen index: the transition-era send path
/// has exactly one BIP44 account, and the asset-lock builder's `account_index`
/// serves the same pinned role there.
const BIP44_ACCOUNT_INDEX: u32 = 0;

/// A built-and-signed Core L1 payment that ALSO carries the bookkeeping a
/// deferred (BIP70/BIP270-style) submission needs: which single account funded
/// it, the height its reservation was stamped at, and the key-wallet token
/// stamped onto the reserved inputs.
///
/// This is the shape [`CoreWallet::finalize_signed_payment_from_funding_path`]
/// returns, and the bridge between the two previously-disconnected send flows:
/// the funding-path selector (the only one that can reach a
/// `DashpayReceivingFunds` account) and the reservation registry that mints
/// broadcast/release tokens.
///
/// Holding one of these means the selected inputs are **reserved**. Either
/// register it with [`SignedPaymentRegistry::register_funded_by`] — which takes
/// over that responsibility and returns a token — or release it with
/// [`CoreWallet::abandon_payment`]. Dropping it without doing either strands the
/// reservation until key-wallet's TTL backstop reclaims it.
///
/// [`SignedPaymentRegistry::register_funded_by`]:
///     crate::SignedPaymentRegistry::register_funded_by
#[derive(Debug, Clone)]
pub struct FinalizedCorePayment {
    /// The signed transaction.
    pub transaction: Transaction,
    /// The fee paid, in duffs — derived from the transaction itself
    /// (`inputs − outputs`), the same ground truth [`SignedCorePayment::fee`]
    /// uses.
    pub fee: u64,
    /// Duffs returned to the wallet's BIP44 change address (0 when the build
    /// produced no change output).
    pub change_amount: u64,
    /// The ONE account the inputs were selected from and reserved in, as its
    /// RESOLVED account-level derivation path — never the caller's `None`. A
    /// later release must name this account, not the default BIP44 one.
    pub funding: FundingAccountRef,
    /// The wallet's `last_processed_height` captured in the funding critical
    /// section, i.e. the exact clock `set_current_height` stamped the
    /// reservation with. The registry's age guard must baseline off this — see
    /// [`SignedCoreTransaction::reservation_height`](crate::SignedCoreTransaction::reservation_height).
    pub reservation_height: u32,
    /// The key-wallet [`ReservationToken`](key_wallet::ReservationToken) stamped
    /// onto the selected inputs, so a later release is *owner-guarded* and frees
    /// only inputs this build still owns (`dashpay/platform#4185`). `None` only
    /// if the build reserved nothing, which the funded path never does.
    pub reservation_token: Option<KeyWalletReservationToken>,
}

/// A built-and-signed Core L1 payment, ready to be committed/broadcast by the
/// caller (dashj during the transition, or a later SDK-broadcast mode).
#[derive(Debug, Clone)]
pub struct SignedCorePayment {
    /// The signed transaction. Serialize with
    /// [`consensus::serialize`](dashcore::consensus::serialize) for the raw
    /// wire bytes the caller hands to its broadcaster.
    pub transaction: Transaction,
    /// The fee paid, in duffs, computed from the encoded size of the *signed*
    /// transaction.
    pub fee: u64,
    /// Duffs returned to the wallet's change address (0 when the build produced
    /// no change output — an exact-match selection or a dust-only remainder
    /// folded into the fee).
    pub change_amount: u64,
}

impl<B: TransactionBroadcaster + ?Sized> CoreWallet<B> {
    /// Build and sign a standard Core L1 payment to `outputs`, funding it from
    /// the **single** funds account named by `funding_path`, and return the
    /// signed transaction plus its fee and change amount. Does **not** broadcast
    /// and does **not** persist a debit (see the module docs for the persistence
    /// contract).
    ///
    /// ## Coin selection — one account, never a union
    ///
    /// Inputs come from exactly one funds account: `None` (the default) funds
    /// from the unmixed BIP44 account at [`BIP44_ACCOUNT_INDEX`], and
    /// `Some(path)` funds strictly from the one funds account whose
    /// account-level derivation path equals `path` (e.g. the DIP-9 CoinJoin
    /// account, to spend previously-mixed coins deliberately). There is **no
    /// union across accounts and no privacy-domain consent gate** — the caller
    /// names exactly one funding source, so there is nothing to consent to. If
    /// that account cannot cover the payment (+ fee) the build fails with
    /// [`PlatformWalletError::PaymentInsufficientFunds`] rather than silently
    /// topping up from another account; that failure is the point, not a
    /// limitation. See [`crate::wallet::funding_privacy`] for why
    /// (dashpay/platform#4073, blocked and re-scoped by #4184).
    ///
    /// Watch-only `DashpayExternalAccount`s can never fund a payment — their
    /// coins belong to a contact and the local mnemonic holds no key for
    /// them — so naming one explicitly is refused rather than silently ignored.
    ///
    /// Change routes to the BIP44 account at [`BIP44_ACCOUNT_INDEX`], which for
    /// the default funding path is the funding account itself. When an explicit
    /// non-Standard account (CoinJoin / DashPay-receiving) funds the payment,
    /// key-wallet cannot derive change on it at all, so the BIP44 sink is
    /// structural — the same change model the approved asset-lock builder uses.
    ///
    /// `LargestFirst` selection is used deliberately (not the builder default
    /// `BranchAndBound`): a CoinJoin account can hold many small mixed
    /// denominations, and `BranchAndBound`'s exact-match subset-sum is
    /// exponential over them (the same hang the asset-lock path avoids).
    /// `LargestFirst`'s linear greedy accumulator also minimizes the input
    /// count — fewer signer round-trips and a smaller tx/fee.
    ///
    /// ## Parameters
    ///
    /// * `outputs` — the recipient `(address, amount_duffs)` pairs. Must be
    ///   non-empty and every amount must be positive.
    /// * `fee_per_kb` — fee rate in duffs/kB, or `None` for the default
    ///   (`1000`).
    /// * `signer` — the ECDSA signer that produces each input's P2PKH signature
    ///   (the Keychain/Keystore-backed `MnemonicResolverCoreSigner` in
    ///   production). No private key crosses the boundary.
    /// * `funding_path` — the account-level derivation path of the SINGLE funds
    ///   account whose UTXOs fund the payment. `None` (the default) funds from
    ///   the unmixed BIP44 account. Mirrors
    ///   `AssetLockManager::build_asset_lock_transaction`'s parameter of the
    ///   same name (dashpay/platform#4184).
    pub async fn build_signed_payment<S: Signer>(
        &self,
        outputs: Vec<(DashAddress, u64)>,
        fee_per_kb: Option<u64>,
        signer: &S,
        funding_path: Option<DerivationPath>,
    ) -> Result<SignedCorePayment, PlatformWalletError> {
        let finalized = self
            .finalize_signed_payment_from_funding_path(outputs, fee_per_kb, signer, funding_path)
            .await?;
        Ok(SignedCorePayment {
            transaction: finalized.transaction,
            fee: finalized.fee,
            change_amount: finalized.change_amount,
        })
    }

    /// [`build_signed_payment`](Self::build_signed_payment) with the deferred
    /// submission bookkeeping retained: identical selection, change routing,
    /// signing, and fee/change accounting, but the result also carries the ONE
    /// account the build reserved into, the height its reservation was stamped
    /// at, and key-wallet's owner token for those inputs.
    ///
    /// This is the bridge that lets a **DashPay receiving-funds** balance reach
    /// the reservation/broadcast lifecycle. That lifecycle's other entry point,
    /// [`finalize_transaction`](Self::finalize_transaction), selects accounts by
    /// key-wallet's `AccountTypePreference` (BIP44 / BIP32 / CoinJoin), which has
    /// no variant for a receival account — so before this method a receival
    /// balance could be *signed* (here) or *broadcast with a token* (there), but
    /// never both. Register the result with
    /// [`SignedPaymentRegistry::register_funded_by`](crate::SignedPaymentRegistry::register_funded_by)
    /// to mint the token the broadcast/release pair consumes.
    ///
    /// ## Reservation ownership — the caller MUST discharge it
    ///
    /// On success the selected inputs are reserved in the funding account's own
    /// ledger. Exactly one of these must follow, or the reservation is stranded
    /// until key-wallet's TTL backstop reclaims it (the funds are not lost, but
    /// they are unspendable meanwhile):
    ///
    /// * register it — the registry then owns the release; or
    /// * [`abandon_payment`](Self::abandon_payment) it.
    ///
    /// Every invariant [`build_signed_payment`](Self::build_signed_payment)
    /// documents holds unchanged, because that method is now a projection of
    /// this one: exactly one funding account (never a union), change to the
    /// unmixed BIP44 account, watch-only accounts refused even when named
    /// explicitly, and a shortfall reported against the SELECTED account alone.
    pub async fn finalize_signed_payment_from_funding_path<S: Signer>(
        &self,
        outputs: Vec<(DashAddress, u64)>,
        fee_per_kb: Option<u64>,
        signer: &S,
        funding_path: Option<DerivationPath>,
    ) -> Result<FinalizedCorePayment, PlatformWalletError> {
        if outputs.is_empty() {
            return Err(PlatformWalletError::TransactionBuild(
                "at least one output is required".to_string(),
            ));
        }
        if outputs.iter().any(|(_, amount)| *amount == 0) {
            return Err(PlatformWalletError::TransactionBuild(
                "every output amount must be greater than zero".to_string(),
            ));
        }

        // Checked aggregation, bounded by MAX_MONEY. key-wallet sums the same
        // amounts with unchecked `u64` arithmetic while building, so an
        // unchecked total here would wrap in release builds (four outputs of
        // `1 << 62` sum to exactly 2^64) and let selection fund only the fee
        // while retaining four enormous outputs — a signed transaction
        // consensus rejects, with meaningless fee/change metadata. In an
        // overflow-checking build the same input panics inside the `extern "C"`
        // FFI frame, where the JNI guard cannot recover it.
        let outputs_total = outputs
            .iter()
            .try_fold(0u64, |total, (_, amount)| total.checked_add(*amount))
            .filter(|total| *total <= MAX_MONEY)
            .ok_or_else(|| {
                PlatformWalletError::TransactionBuild(format!(
                    "output amounts overflow or exceed MAX_MONEY ({MAX_MONEY} duffs)"
                ))
            })?;

        // Bound the caller-supplied fee rate for the same reason: key-wallet's
        // `FeeRate::calculate_fee` computes `sat_per_kb * size_bytes` with
        // unchecked `u64` multiplication, so a rate near `u64::MAX` (the public
        // Kotlin/FFI APIs accept any non-negative `Long`) panics in an
        // overflow-checking Android build, or wraps in release — turning an
        // astronomical requested rate into a tiny fee.
        let fee_per_kb = fee_per_kb.unwrap_or(DEFAULT_FEE_PER_KB);
        if fee_per_kb > MAX_FEE_PER_KB {
            return Err(PlatformWalletError::TransactionBuild(format!(
                "fee rate {fee_per_kb} duffs/kB exceeds the maximum {MAX_FEE_PER_KB}"
            )));
        }

        let mut wm = self.wallet_manager.write().await;
        let (wallet, info) = wm
            .get_wallet_and_info_mut(&self.wallet_id)
            .ok_or_else(|| PlatformWalletError::WalletNotFound(hex::encode(self.wallet_id)))?;

        let height = info.core_wallet.last_processed_height();
        let network = info.core_wallet.network();
        let fee_rate = FeeRate::new(fee_per_kb);

        // ------------------------------------------------------------------
        // REGRESSION NOTE (dashpay/platform#4073 → #4184 → #4247)
        //
        // This selection block previously unioned every signable funds account
        // and ran LargestFirst over the combined set, with BIP44 change — which
        // irreversibly links ordinary, CoinJoin, and DashPay-receiving coins in
        // one on-chain transaction. Reviewer shumkov blocked exactly that on
        // PR #4184 (2026-07-21); the single-selected-account redesign (commit
        // 4d3e1322bc) was signed off 2026-07-23.
        //
        // The send-raw-tx code was written on an older integration line BEFORE
        // that re-scope. On 2026-07-28 it was extracted into PR #4247 and
        // pushed without being diffed against the design decision already
        // settled on the sibling PR; the extraction was treated as mechanical,
        // and the recorded decision was simply not consulted.
        //
        // Nothing surfaced it: the code compiled, `cargo check` passed, and the
        // PR carried a test (`payment_funds_from_bip44_and_coinjoin_union`)
        // asserting the union as correct behavior. Two automated reviewers then
        // flagged six unrelated mechanical defects while treating the union as
        // intended design — one blocker complained that "secondary-account
        // inputs are not reserved", accepting cross-account funding as premise.
        //
        // INVARIANT: single selected account; never union funding accounts;
        // default unmixed BIP44. A compile-clean, review-passed change is NOT
        // sufficient evidence of correctness here — see
        // `crate::wallet::funding_privacy` and its guardrail tests.
        // ------------------------------------------------------------------

        // Resolve the account-level path of the unmixed BIP44 account: both the
        // default funding source and the change sink.
        let bip44_path = info
            .core_wallet
            .accounts
            .standard_bip44_accounts
            .get(&BIP44_ACCOUNT_INDEX)
            .ok_or_else(|| {
                PlatformWalletError::TransactionBuild(format!(
                    "BIP44 account {BIP44_ACCOUNT_INDEX} not found for payment funding"
                ))
            })?
            .managed_account_type()
            .to_account_type()
            .derivation_path(network)
            .map_err(|e| {
                PlatformWalletError::TransactionBuild(format!(
                    "failed to derive the unmixed BIP44 account-level path: {e}"
                ))
            })?;
        let funding_path = funding_path.unwrap_or_else(|| bip44_path.clone());
        let funds_from_change_account = funding_path == bip44_path;

        // The xpub-bearing BIP44 account: the change sink, and the fallback
        // signing-side account. Cloned so no immutable borrow of `wallet` is
        // held across the mutable `info` borrow below.
        let bip44_acc = wallet
            .get_bip44_account(BIP44_ACCOUNT_INDEX)
            .ok_or_else(|| {
                PlatformWalletError::TransactionBuild(format!(
                    "BIP44 account {BIP44_ACCOUNT_INDEX} not found for payment change routing"
                ))
            })?
            .clone();

        // Derive an explicit BIP44 change address ONLY when the funding account
        // is not the BIP44 sink itself: `set_funding` already derives change on
        // the funding account, which is correct (and consumes no extra pool
        // index) in the default case, but fails and is swallowed to `None` for a
        // non-Standard CoinJoin / DashPay account. Taken before the funding
        // account's `&mut` below — two accounts of the same collection cannot
        // both be borrowed mutably at once.
        let change_addr: Option<DashAddress> = if funds_from_change_account {
            None
        } else {
            let change_acc = info
                .core_wallet
                .accounts
                .standard_bip44_accounts
                .get_mut(&BIP44_ACCOUNT_INDEX)
                .ok_or_else(|| {
                    PlatformWalletError::TransactionBuild(format!(
                        "managed BIP44 account {BIP44_ACCOUNT_INDEX} not found for payment \
                         change routing"
                    ))
                })?;
            Some(
                change_acc
                    .next_change_address(Some(&bip44_acc.account_xpub), true)
                    .map_err(|e| {
                        PlatformWalletError::TransactionBuild(format!(
                            "failed to derive change address on BIP44 account \
                             {BIP44_ACCOUNT_INDEX}: {e}"
                        ))
                    })?,
            )
        };

        // The funding account's OWN wallet-level `Account`. `set_funding` calls
        // `funds_acc.next_change_address(Some(&acc.account_xpub))` before the
        // `set_change_address` override, so `acc` must be the funding account —
        // passing the BIP44 xpub for an explicitly-selected BIP32 account would
        // record a change entry derived from the wrong xpub into that account's
        // pool (dashpay/platform#4184 review). Falls back to `bip44_acc` when no
        // wallet-level account matches, preserving the default behavior.
        let funding_wallet_acc = wallet
            .all_accounts()
            .into_iter()
            .find(|a| {
                a.derivation_path()
                    .map(|p| p == funding_path)
                    .unwrap_or(false)
            })
            .unwrap_or(&bip44_acc);

        // Locate the ONE managed funds account whose account-level path equals
        // `funding_path`, MUTABLY, so `set_funding` reserves the selected inputs
        // in that account's OWN reservation ledger. Watch-only
        // `DashpayExternalAccount`s are never fundable (the local mnemonic
        // cannot sign them) — refuse even when named explicitly.
        //
        // PRIVACY-DOMAIN-OK: this iterates funds accounts only to LOOK ONE UP by
        // derivation path. Exactly one account is selected and it alone funds
        // the transaction; nothing is accumulated across accounts.
        let mut selected: Option<&mut ManagedCoreFundsAccount> = None;
        for acc in info.core_wallet.accounts.all_funding_accounts_mut() {
            let acc_path = acc
                .managed_account_type()
                .to_account_type()
                .derivation_path(network)
                .map_err(|e| {
                    PlatformWalletError::TransactionBuild(format!(
                        "failed to derive account-level path for a funds account: {e}"
                    ))
                })?;
            if acc_path != funding_path {
                continue;
            }
            if !is_signable_funding_account(acc.managed_account_type()) {
                return Err(PlatformWalletError::TransactionBuild(format!(
                    "funding derivation path {funding_path} names a watch-only account whose \
                     coins the local wallet cannot sign; choose a signable funds account"
                )));
            }
            selected = Some(acc);
            break;
        }
        let selected = selected.ok_or_else(|| {
            PlatformWalletError::TransactionBuild(format!(
                "no spendable funds account matches funding derivation path {funding_path}"
            ))
        })?;

        // One immutable pass over the SELECTED account, building:
        //   (a) an owned `Address -> DerivationPath` resolver, so signing can
        //       resolve a key for every selected input without holding an
        //       account borrow across the signer await;
        //   (b) an `OutPoint -> value` map for the post-build fee/change figures;
        //   (c) the account's selectable total, for a typed shortfall error.
        let mut path_map: HashMap<DashAddress, DerivationPath> = HashMap::new();
        let mut input_value: HashMap<OutPoint, u64> = HashMap::new();
        let mut selectable_value: u64 = 0;
        for utxo in selected.spendable_utxos(height) {
            selectable_value = selectable_value.saturating_add(utxo.value());
            input_value.insert(utxo.outpoint, utxo.value());
            if let Some(path) = selected.address_derivation_path(&utxo.address) {
                path_map.insert(utxo.address.clone(), path);
            }
        }

        // Seed the selected account (inputs + reservations + its own change
        // address), override the change sink when the funding account cannot
        // derive change, then add the recipient outputs. The `&mut` borrow ends
        // with `set_funding`; the returned builder owns cloned inputs /
        // reservations / change address, so no account borrow is held across the
        // signer await below.
        let builder = {
            let mut builder = TransactionBuilder::new()
                .set_fee_rate(fee_rate)
                .set_current_height(height)
                // See the doc-comment: LargestFirst, not the default
                // BranchAndBound, to keep CoinJoin's many small denominations
                // from blowing up the exact-match subset-sum search.
                .set_selection_strategy(SelectionStrategy::LargestFirst)
                .set_funding(selected, funding_wallet_acc);
            if let Some(addr) = change_addr {
                builder = builder.set_change_address(addr);
            }
            for (address, amount) in &outputs {
                builder = builder.add_output(address, *amount);
            }
            builder
        };

        // `build_signed_reserved`, not `build_signed`: identical build, but it
        // also hands back the key-wallet ReservationToken it stamped onto the
        // selected inputs. `build_signed` is literally this call with the token
        // discarded — which is exactly what left a deferred payment unable to
        // release owner-guarded. Keeping the token is what makes a registered
        // (tokened) payment releasable without risking a TTL-swept, re-reserved
        // input belonging to another build (`dashpay/platform#4185`).
        let (transaction, _estimated_fee, reservation_token) = builder
            .build_signed_reserved(signer, move |addr| path_map.get(&addr).cloned())
            .await
            .map_err(|e| map_send_builder_error(e, selectable_value, outputs_total))?;

        // Derive fee and change from the transaction itself — the ground truth
        // that is always self-consistent (`fee + outputs + change == inputs`).
        // We do NOT use `build_signed`'s returned fee: it recomputes the fee
        // from the *signed* size, but the change output was already sized with
        // the pre-sign estimate, and ECDSA signatures vary in encoded length —
        // so the recomputed figure can differ by a few duffs from the fee the
        // wallet actually pays (`inputs − outputs`).
        //
        // `total_out` is the sum of every output; the only non-recipient output
        // a plain payment (no special payload) can carry is the single change
        // output back to the BIP44 sink, so `change = total_out − outputs`.
        // Any selected input we somehow can't price (impossible — every
        // spendable UTXO was recorded above) counts as 0, so `fee` is over-
        // rather than under-reported.
        let selected_input_value: u64 = transaction
            .input
            .iter()
            .map(|txin| input_value.get(&txin.previous_output).copied().unwrap_or(0))
            .sum();
        let total_out: u64 = transaction.output.iter().map(|o| o.value).sum();
        let fee = selected_input_value.saturating_sub(total_out);
        let change_amount = total_out.saturating_sub(outputs_total);

        Ok(FinalizedCorePayment {
            transaction,
            fee,
            change_amount,
            // The RESOLVED path, never the caller's `None`: a release must name
            // the account the inputs are actually reserved in. For a default
            // build that is the unmixed BIP44 account's own path, so the release
            // still lands on BIP44 — but by the same identity the selector used,
            // not by a separate assumption that could drift.
            funding: FundingAccountRef::Path(funding_path),
            reservation_height: height,
            reservation_token,
        })
    }

    /// Release the funding reservation of a
    /// [`FinalizedCorePayment`] the caller has decided not to submit — the
    /// path-funded counterpart of
    /// [`abandon_transaction`](Self::abandon_transaction).
    ///
    /// Owner-guarded and generation-bound, like every other release here. Use it
    /// on any failure between the build and a successful
    /// [`register_funded_by`](crate::SignedPaymentRegistry::register_funded_by);
    /// once registered, the registry owns the release instead.
    pub async fn abandon_payment(&self, payment: &FinalizedCorePayment) {
        self.release_reservation_for(
            &payment.funding,
            &payment.transaction,
            payment.reservation_token,
        )
        .await;
    }
}

/// Map a key-wallet [`BuilderError`] to a [`PlatformWalletError`], promoting the
/// two shortfall shapes to the typed [`PlatformWalletError::PaymentInsufficientFunds`]
/// so the exact `available`/`required` duff amounts survive.
///
/// `available` is the **selected account's** spendable total, deliberately —
/// never a wallet-wide figure. Reporting a wallet-wide "available" against a
/// single-account shortfall would invite the caller to retry with a larger
/// amount that can only succeed by crossing privacy domains, which this
/// primitive will not do (see [`crate::wallet::funding_privacy`]). `required` is
/// at least the outputs total; a coin-selection error already carries the
/// fee-inclusive figure, which we prefer when present.
fn map_send_builder_error(
    error: BuilderError,
    available_in_account: u64,
    outputs_total: u64,
) -> PlatformWalletError {
    match error {
        BuilderError::InsufficientFunds { required, .. } => {
            PlatformWalletError::PaymentInsufficientFunds {
                available: available_in_account,
                required: required.max(outputs_total),
            }
        }
        BuilderError::CoinSelection(SelectionError::InsufficientFunds { required, .. }) => {
            PlatformWalletError::PaymentInsufficientFunds {
                available: available_in_account,
                required: required.max(outputs_total),
            }
        }
        BuilderError::CoinSelection(SelectionError::NoUtxosAvailable) => {
            PlatformWalletError::PaymentInsufficientFunds {
                available: available_in_account,
                required: outputs_total,
            }
        }
        other => PlatformWalletError::TransactionBuild(format!("payment build failed: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use dashcore::hashes::Hash;
    use dashcore::{Address as DashAddress, Network, OutPoint, TxOut, Txid};
    use key_wallet::account::account_type::StandardAccountType;
    use key_wallet::account::AccountType;
    use key_wallet::bip32::DerivationPath;
    use key_wallet::managed_account::ManagedCoreFundsAccount;
    use key_wallet::Utxo;

    use crate::test_support::{
        funded_wallet_manager, split_funded_wallet_manager, split_funded_wallet_manager_dashpay,
        AlwaysRejectedBroadcaster, DashpayLeg,
    };
    use crate::wallet::core::balance::WalletBalance;
    use crate::wallet::core::CoreWallet;
    use crate::wallet::platform_wallet::WalletId;
    use crate::PlatformWalletError;

    use super::{FundingAccountRef, SignedCorePayment};

    /// A `CoreWallet` over a manager fixture. The send path never broadcasts,
    /// so the broadcaster is irrelevant (and the balance handle is unused by
    /// build — a fresh one is fine for the split fixtures that don't return it).
    fn core_wallet(
        wallet_manager: Arc<tokio::sync::RwLock<key_wallet_manager::WalletManager<crate::wallet::platform_wallet::PlatformWalletInfo>>>,
        wallet_id: WalletId,
        balance: Arc<WalletBalance>,
    ) -> CoreWallet<AlwaysRejectedBroadcaster> {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        CoreWallet::new(
            sdk,
            wallet_manager,
            wallet_id,
            Arc::new(AlwaysRejectedBroadcaster),
            balance,
        )
    }

    fn recipient(seed: u8) -> DashAddress {
        DashAddress::dummy(Network::Testnet, seed as usize)
    }

    /// Every input of a signed tx must carry a non-empty scriptSig (proof each
    /// selected input was actually signed by the per-account resolver).
    fn assert_all_inputs_signed(payment: &SignedCorePayment) {
        for (i, txin) in payment.transaction.input.iter().enumerate() {
            assert!(
                !txin.script_sig.is_empty(),
                "input {i} was left unsigned (empty scriptSig)"
            );
        }
    }

    /// A single-account BIP44 payment: the recipient output is present with the
    /// exact value, a fee is charged, and the change amount is exactly
    /// selected_input − output − fee (here the whole 0.1 DASH rides on one
    /// input, so change ≈ 0.1 − amount − fee).
    #[tokio::test]
    async fn bip44_payment_has_correct_output_change_and_fee() {
        let (wm, wallet_id, balance, signer) =
            funded_wallet_manager(StandardAccountType::BIP44Account).await;
        let core = core_wallet(wm, wallet_id, balance);

        let to = recipient(42);
        let amount = 1_000_000u64;
        let payment = core
            .build_signed_payment(vec![(to.clone(), amount)], None, &signer, None)
            .await
            .expect("build should succeed with 0.1 DASH funded");

        // Recipient output present with the exact value.
        let recipient_out = payment
            .transaction
            .output
            .iter()
            .find(|o| o.script_pubkey == to.script_pubkey());
        assert_eq!(
            recipient_out.map(|o| o.value),
            Some(amount),
            "recipient output must carry the requested amount"
        );

        // A fee was charged and change is exactly input − output − fee.
        assert!(payment.fee > 0, "a non-zero fee should be charged");
        assert_eq!(
            payment.change_amount,
            10_000_000 - amount - payment.fee,
            "change must be the single input minus the output minus the fee"
        );
        // The change output pays the leftover back to the wallet.
        assert!(
            payment
                .transaction
                .output
                .iter()
                .any(|o| o.value == payment.change_amount),
            "a change output equal to change_amount should exist"
        );
        assert_all_inputs_signed(&payment);
    }

    /// Snapshot the BIP44 and CoinJoin outpoints of a split fixture, plus the
    /// CoinJoin account's account-level derivation path (the `funding_path` a
    /// caller passes to spend previously-mixed coins deliberately).
    async fn split_account_outpoints_and_coinjoin_path(
        wm: &Arc<tokio::sync::RwLock<key_wallet_manager::WalletManager<crate::wallet::platform_wallet::PlatformWalletInfo>>>,
        wallet_id: &WalletId,
    ) -> (HashSet<OutPoint>, HashSet<OutPoint>, DerivationPath) {
        use key_wallet::managed_account::managed_account_trait::ManagedAccountTrait;

        let guard = wm.read().await;
        let (_, info) = guard.get_wallet_and_info(wallet_id).expect("wallet present");
        let network = info.core_wallet.network();
        let bip44 = info
            .core_wallet
            .accounts
            .standard_bip44_accounts
            .get(&0)
            .map(|a| a.utxos.keys().copied().collect())
            .unwrap_or_default();
        let coinjoin_acc = info
            .core_wallet
            .accounts
            .coinjoin_accounts
            .get(&0)
            .expect("coinjoin account 0 present");
        let coinjoin = coinjoin_acc.utxos.keys().copied().collect();
        let path = coinjoin_acc
            .managed_account_type()
            .to_account_type()
            .derivation_path(network)
            .expect("coinjoin account-level path");
        (bip44, coinjoin, path)
    }

    /// **Replaces `payment_funds_from_bip44_and_coinjoin_union`**, which asserted
    /// the blocked union behavior as correct (dashpay/platform#4247; see the
    /// regression note in `build_signed_payment`).
    ///
    /// The DEFAULT funding path must never select CoinJoin (or any other
    /// non-BIP44 domain) coins, even when BIP44 alone cannot cover the payment.
    /// Failing is the correct outcome — matching the approved asset-lock
    /// contract (`shielded_asset_lock_never_unions_accounts`): a shortfall is
    /// reported as a typed error rather than silently satisfied by crossing a
    /// privacy domain, because the cross-domain link would be irreversible while
    /// the failure is merely retryable with an explicit `funding_path`.
    #[tokio::test]
    async fn default_funding_never_selects_other_domains() {
        // 0.09 DASH on BIP44, 0.09 on CoinJoin; ask 0.15 → only a union covers it.
        let (wm, wallet_id, signer) = split_funded_wallet_manager(9_000_000, 9_000_000).await;
        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));

        let result = core
            .build_signed_payment(vec![(recipient(7), 15_000_000)], None, &signer, None)
            .await;

        match result {
            Err(PlatformWalletError::PaymentInsufficientFunds {
                available,
                required,
            }) => {
                assert_eq!(
                    available, 9_000_000,
                    "available must reflect ONLY the BIP44 account, never the \
                     wallet-wide union"
                );
                assert!(
                    required >= 15_000_000,
                    "required {required} should be at least the requested amount"
                );
            }
            other => panic!(
                "the default path must not union BIP44 with CoinJoin — expected \
                 PaymentInsufficientFunds, got {other:?}"
            ),
        }
    }

    /// The default path funds happily from BIP44 when BIP44 alone suffices, and
    /// still leaves the CoinJoin coins untouched.
    #[tokio::test]
    async fn default_funding_selects_strictly_within_bip44() {
        // 0.2 DASH on BIP44, 0.09 on CoinJoin; ask 0.15 → BIP44 alone covers it.
        let (wm, wallet_id, signer) = split_funded_wallet_manager(20_000_000, 9_000_000).await;
        let (bip44_ops, coinjoin_ops, _) =
            split_account_outpoints_and_coinjoin_path(&wm, &wallet_id).await;

        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));
        let payment = core
            .build_signed_payment(vec![(recipient(7), 15_000_000)], None, &signer, None)
            .await
            .expect("0.15 DASH is fundable from the 0.2 DASH BIP44 account");

        let spent: HashSet<OutPoint> = payment
            .transaction
            .input
            .iter()
            .map(|i| i.previous_output)
            .collect();
        assert!(
            spent.iter().all(|op| bip44_ops.contains(op)),
            "every input must come from BIP44, spent {spent:?}"
        );
        assert!(
            !spent.iter().any(|op| coinjoin_ops.contains(op)),
            "the default path must never reach CoinJoin coins, spent {spent:?}"
        );
        assert_all_inputs_signed(&payment);
    }

    /// An explicitly-passed CoinJoin path selects strictly from that account and
    /// nothing else — the caller-consented, single-domain half of the #4184
    /// contract. Change still lands on BIP44 because key-wallet cannot derive a
    /// change address on a non-Standard account; that is structural, not a
    /// co-spend.
    #[tokio::test]
    async fn explicit_coinjoin_path_selects_only_coinjoin() {
        // 0.09 DASH on BIP44 (short), 0.2 on CoinJoin; take 0.15 from CoinJoin.
        let (wm, wallet_id, signer) = split_funded_wallet_manager(9_000_000, 20_000_000).await;
        let (bip44_ops, coinjoin_ops, coinjoin_path) =
            split_account_outpoints_and_coinjoin_path(&wm, &wallet_id).await;

        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));
        let payment = core
            .build_signed_payment(
                vec![(recipient(7), 15_000_000)],
                None,
                &signer,
                Some(coinjoin_path),
            )
            .await
            .expect("the named CoinJoin account covers 0.15 DASH");

        let spent: HashSet<OutPoint> = payment
            .transaction
            .input
            .iter()
            .map(|i| i.previous_output)
            .collect();
        assert!(!spent.is_empty(), "the payment must have selected inputs");
        assert!(
            spent.iter().all(|op| coinjoin_ops.contains(op)),
            "every input must come from the named CoinJoin account, spent {spent:?}"
        );
        assert!(
            !spent.iter().any(|op| bip44_ops.contains(op)),
            "an explicit CoinJoin path must not pull BIP44 inputs, spent {spent:?}"
        );
        // Change is returned to the transparent BIP44 sink.
        assert!(
            payment.change_amount > 0,
            "spending a 0.2 DASH UTXO for 0.15 DASH must leave change"
        );
        assert_all_inputs_signed(&payment);
    }

    /// A shortfall inside the SELECTED account surfaces as the typed
    /// [`PlatformWalletError::PaymentInsufficientFunds`], with `available`
    /// reflecting only that account — never a wallet-wide union total, which
    /// would invite a retry that can only succeed by crossing domains.
    #[tokio::test]
    async fn selected_account_shortfall_is_typed() {
        let (wm, wallet_id, signer) = split_funded_wallet_manager(9_000_000, 9_000_000).await;
        let (_, _, coinjoin_path) =
            split_account_outpoints_and_coinjoin_path(&wm, &wallet_id).await;
        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));

        let result = core
            .build_signed_payment(
                vec![(recipient(7), 100_000_000)],
                None,
                &signer,
                Some(coinjoin_path),
            )
            .await;

        match result {
            Err(PlatformWalletError::PaymentInsufficientFunds {
                available,
                required,
            }) => {
                assert_eq!(
                    available, 9_000_000,
                    "available must reflect only the named CoinJoin account"
                );
                assert!(
                    required >= 100_000_000,
                    "required {required} should be at least the requested amount"
                );
            }
            other => panic!("expected PaymentInsufficientFunds, got {other:?}"),
        }
    }

    /// A `funding_path` that names no funds account is a hard error — never a
    /// silent fallback to the default account, which would fund the payment
    /// from coins the caller did not choose.
    #[tokio::test]
    async fn unknown_funding_path_is_rejected() {
        use std::str::FromStr;

        let (wm, wallet_id, balance, signer) =
            funded_wallet_manager(StandardAccountType::BIP44Account).await;
        let core = core_wallet(wm, wallet_id, balance);

        let nowhere = DerivationPath::from_str("m/44'/5'/77'").expect("valid path");
        let result = core
            .build_signed_payment(
                vec![(recipient(7), 1_000_000)],
                None,
                &signer,
                Some(nowhere),
            )
            .await;
        assert!(
            matches!(result, Err(PlatformWalletError::TransactionBuild(_))),
            "an unmatched funding path must fail, got {result:?}"
        );
    }

    /// A watch-only `DashpayExternalAccount` (a contact's addresses, which this
    /// wallet cannot sign) is EXCLUDED from coin selection: its UTXO is never
    /// spent, and its value is not counted toward the selectable total.
    #[tokio::test]
    async fn watch_only_external_account_is_excluded() {
        // BIP44 holds 0.1 DASH; a watch-only external account holds 1.0 DASH.
        let (wm, wallet_id, _balance, signer) =
            funded_wallet_manager(StandardAccountType::BIP44Account).await;

        let watch_only_outpoint = OutPoint {
            txid: Txid::from_byte_array([0x9au8; 32]),
            vout: 0,
        };
        {
            let mut guard = wm.write().await;
            let (wallet, info) = guard
                .get_wallet_mut_and_info_mut(&wallet_id)
                .expect("wallet present");

            // Reuse the wallet's own BIP44 xpub as a stand-in "contact xpub":
            // the exclusion happens before any address derivation, so any valid
            // xpub suffices to construct the funds-bearing external account.
            let contact_xpub = wallet
                .accounts
                .standard_bip44_accounts
                .get(&0)
                .expect("bip44 account 0")
                .account_xpub;
            let account_type = AccountType::DashpayExternalAccount {
                index: 0,
                user_identity_id: [1u8; 32],
                friend_identity_id: [2u8; 32],
            };
            let account = key_wallet::Account {
                parent_wallet_id: Some(wallet_id),
                account_type,
                network: Network::Testnet,
                account_xpub: contact_xpub,
                is_watch_only: true,
            };
            let mut managed = ManagedCoreFundsAccount::from_account(&account);

            // Insert a large spendable UTXO directly (arbitrary address — the
            // account is skipped before its addresses are ever consulted).
            let addr = recipient(200);
            let utxo = Utxo {
                outpoint: watch_only_outpoint,
                txout: TxOut {
                    value: 100_000_000,
                    script_pubkey: addr.script_pubkey(),
                },
                address: addr,
                height: 1,
                is_coinbase: false,
                is_confirmed: true,
                is_instantlocked: false,
                is_locked: false,
                is_trusted: false,
            };
            managed.utxos.insert(utxo.outpoint, utxo);
            info.core_wallet
                .accounts
                .insert_funds_bearing_account(managed)
                .expect("insert watch-only external account");
        }

        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));

        // Ask for 0.5 DASH: covered only if the 1.0-DASH watch-only UTXO were
        // spendable. Since it is excluded, the build must fail — and the
        // reported `available` must be just the 0.1-DASH BIP44 slice.
        let result = core
            .build_signed_payment(vec![(recipient(7), 50_000_000)], None, &signer, None)
            .await;
        match result {
            Err(PlatformWalletError::PaymentInsufficientFunds { available, .. }) => {
                assert_eq!(
                    available, 10_000_000,
                    "watch-only value must be excluded from the selectable total"
                );
            }
            other => panic!("expected PaymentInsufficientFunds, got {other:?}"),
        }

        // And a payment that the 0.1-DASH BIP44 slice CAN cover must never spend
        // the watch-only outpoint.
        let payment = core
            .build_signed_payment(vec![(recipient(7), 1_000_000)], None, &signer, None)
            .await
            .expect("0.01 DASH is fundable from the BIP44 slice alone");
        assert!(
            payment
                .transaction
                .input
                .iter()
                .all(|i| i.previous_output != watch_only_outpoint),
            "the watch-only UTXO must never be selected as an input"
        );
        assert_all_inputs_signed(&payment);
    }

    /// The wallet's OWN per-generation balance handle.
    ///
    /// Every reservation release is generation-bound: it acts only if the
    /// `CoreWallet`'s balance `Arc` is pointer-equal to the one registered under
    /// the wallet id (`Arc::ptr_eq` in `release_reservation_for`). The split
    /// fixtures don't hand their balance back, so a `CoreWallet` built with a
    /// fresh `WalletBalance::new()` is — correctly — treated as a *different*
    /// generation and every release is skipped. Tests that assert release
    /// behaviour must therefore build on this handle, not a fresh one.
    async fn wallet_generation(
        wm: &Arc<tokio::sync::RwLock<key_wallet_manager::WalletManager<crate::wallet::platform_wallet::PlatformWalletInfo>>>,
        wallet_id: &WalletId,
    ) -> Arc<WalletBalance> {
        let guard = wm.read().await;
        let (_, info) = guard.get_wallet_and_info(wallet_id).expect("wallet present");
        Arc::clone(&info.balance)
    }

    /// Snapshot a DashPay fixture's BIP44 outpoints, its DashPay
    /// receiving-funds outpoints, and that receival account's account-level
    /// derivation path — the `funding_path` a caller round-trips from the
    /// account-balance enumeration to spend a receival balance.
    async fn dashpay_outpoints_and_receival_path(
        wm: &Arc<tokio::sync::RwLock<key_wallet_manager::WalletManager<crate::wallet::platform_wallet::PlatformWalletInfo>>>,
        wallet_id: &WalletId,
    ) -> (HashSet<OutPoint>, HashSet<OutPoint>, DerivationPath) {
        use key_wallet::managed_account::managed_account_trait::ManagedAccountTrait;

        let guard = wm.read().await;
        let (_, info) = guard.get_wallet_and_info(wallet_id).expect("wallet present");
        let network = info.core_wallet.network();
        let bip44 = info
            .core_wallet
            .accounts
            .standard_bip44_accounts
            .get(&0)
            .map(|a| a.utxos.keys().copied().collect())
            .unwrap_or_default();
        let receival_acc = info
            .core_wallet
            .accounts
            .dashpay_receival_accounts
            .values()
            .next()
            .expect("DashPay receiving-funds account present");
        let receival = receival_acc.utxos.keys().copied().collect();
        let path = receival_acc
            .managed_account_type()
            .to_account_type()
            .derivation_path(network)
            .expect("DashPay receiving-funds account-level path");
        (bip44, receival, path)
    }

    /// **The DashPay receival-spend bridge.** A payment finalized from a DashPay
    /// receiving-funds account must (a) select strictly within that account,
    /// (b) sign every input, (c) route change to the BIP44 sink, and (d) come
    /// back with the reservation bookkeeping a deferred broadcast needs — the
    /// resolved funding account and key-wallet's owner token.
    ///
    /// Before this path existed the two halves were disconnected:
    /// `finalize_transaction` mints reservation tokens but selects by
    /// `AccountTypePreference`, which has NO variant for a receival account, so a
    /// receival balance could be signed or tokened but never both.
    #[tokio::test]
    async fn receival_funding_path_selects_signs_and_reserves_in_that_account() {
        // 0.09 DASH on BIP44 (cannot cover 0.15), 0.2 on the receival account.
        let (wm, wallet_id, signer) =
            split_funded_wallet_manager_dashpay(9_000_000, 20_000_000, DashpayLeg::ReceivingFunds)
                .await;
        let (bip44_ops, receival_ops, receival_path) =
            dashpay_outpoints_and_receival_path(&wm, &wallet_id).await;

        let core = core_wallet(wm, wallet_id, Arc::new(WalletBalance::new()));
        let payment = core
            .finalize_signed_payment_from_funding_path(
                vec![(recipient(7), 15_000_000)],
                None,
                &signer,
                Some(receival_path.clone()),
            )
            .await
            .expect("the named DashPay receival account covers 0.15 DASH");

        // (a) Single-account selection: only receival inputs, never BIP44's.
        let spent: HashSet<OutPoint> = payment
            .transaction
            .input
            .iter()
            .map(|i| i.previous_output)
            .collect();
        assert!(!spent.is_empty(), "the payment must have selected inputs");
        assert!(
            spent.iter().all(|op| receival_ops.contains(op)),
            "every input must come from the named receival account, spent {spent:?}"
        );
        assert!(
            !spent.iter().any(|op| bip44_ops.contains(op)),
            "a receival-funded payment must not pull BIP44 inputs, spent {spent:?}"
        );

        // (b) Every input signed by the receival account's own derivation path.
        for (i, txin) in payment.transaction.input.iter().enumerate() {
            assert!(
                !txin.script_sig.is_empty(),
                "input {i} was left unsigned (empty scriptSig)"
            );
        }

        // (c) Change lands on the BIP44 sink — key-wallet derives change only
        // for Standard accounts, so this is structural, not a co-spend.
        assert!(
            payment.change_amount > 0,
            "spending a 0.2 DASH UTXO for 0.15 DASH must leave change"
        );
        assert!(
            payment
                .transaction
                .output
                .iter()
                .any(|o| o.value == payment.change_amount),
            "a change output equal to change_amount should exist"
        );

        // (d) The deferred bookkeeping: the RESOLVED receival path (never the
        // caller's `None`, never a BIP44 default) and key-wallet's owner token.
        match &payment.funding {
            FundingAccountRef::Path(path) => assert_eq!(
                *path, receival_path,
                "the funding account must be recorded as the receival path it \
                 actually selected from"
            ),
            other => panic!("expected a path-named funding account, got {other:?}"),
        }
        assert!(
            payment.reservation_token.is_some(),
            "a funded build must stamp a key-wallet reservation token so a later \
             release is owner-guarded"
        );
    }

    /// The reservation a receival-funded payment takes must be recorded against
    /// the RECEIVAL account — and releasing the registry token must give those
    /// exact inputs back.
    ///
    /// This is the funds-critical half: if the registry recorded the funding
    /// account as BIP44 (the only thing an `AccountTypePreference` could say
    /// about a receival account), the release would free BIP44's reservation and
    /// leave the receival coins locked until key-wallet's TTL — while an
    /// unrelated BIP44 build lost its inputs.
    #[tokio::test]
    async fn receival_reservation_is_held_and_released_against_the_receival_account() {
        use crate::wallet::signed_payment_registry::SignedPaymentRegistry;

        let (wm, wallet_id, signer) =
            split_funded_wallet_manager_dashpay(9_000_000, 20_000_000, DashpayLeg::ReceivingFunds)
                .await;
        let (_, _, receival_path) = dashpay_outpoints_and_receival_path(&wm, &wallet_id).await;
        // The wallet's OWN generation handle — releases are generation-bound and
        // are (correctly) skipped for a foreign one. See [`wallet_generation`].
        let generation = wallet_generation(&wm, &wallet_id).await;
        let core = core_wallet(wm, wallet_id, generation);

        let payment = core
            .finalize_signed_payment_from_funding_path(
                vec![(recipient(7), 15_000_000)],
                None,
                &signer,
                Some(receival_path.clone()),
            )
            .await
            .expect("first receival build succeeds");

        let registry: SignedPaymentRegistry<AlwaysRejectedBroadcaster> =
            SignedPaymentRegistry::new();
        let token = registry
            .register_funded_by(
                core.clone(),
                payment.transaction.clone(),
                payment.funding.clone(),
                Some(payment.reservation_height),
                payment.reservation_token,
            )
            .await;
        assert_eq!(registry.outstanding(), 1, "the token must be registered");

        // The reservation is HELD: the receival account has a single UTXO, so a
        // second build from it cannot re-select the reserved input.
        let blocked = core
            .finalize_signed_payment_from_funding_path(
                vec![(recipient(8), 15_000_000)],
                None,
                &signer,
                Some(receival_path.clone()),
            )
            .await;
        assert!(
            matches!(
                blocked,
                Err(PlatformWalletError::PaymentInsufficientFunds { .. })
            ),
            "the reserved receival input must not be re-selectable while the \
             token is outstanding, got {blocked:?}"
        );

        // Releasing the token returns those exact inputs to the receival
        // account's own selectable pool.
        registry.release(token).await;
        assert_eq!(registry.outstanding(), 0, "release must drop the token");
        let after_release = core
            .finalize_signed_payment_from_funding_path(
                vec![(recipient(9), 15_000_000)],
                None,
                &signer,
                Some(receival_path.clone()),
            )
            .await;
        assert!(
            after_release.is_ok(),
            "releasing the token must return the receival inputs to spendable, \
             got {after_release:?}"
        );

        // And the same reconciliation runs on a definitively rejected broadcast:
        // register the rebuilt payment, broadcast it through the always-rejecting
        // broadcaster, and confirm the reservation was released for an immediate
        // rebuild rather than stranded until the TTL backstop.
        let rebuilt = after_release.expect("rebuilt payment");
        let token = registry
            .register_funded_by(
                core.clone(),
                rebuilt.transaction.clone(),
                rebuilt.funding.clone(),
                Some(rebuilt.reservation_height),
                rebuilt.reservation_token,
            )
            .await;
        let broadcast = registry.broadcast(token, &core).await;
        assert!(
            broadcast.is_err(),
            "the always-rejecting broadcaster must surface a failure"
        );
        assert_eq!(
            registry.outstanding(),
            0,
            "a consumed token must not remain registered"
        );
        assert!(
            core.finalize_signed_payment_from_funding_path(
                vec![(recipient(10), 15_000_000)],
                None,
                &signer,
                Some(receival_path),
            )
            .await
            .is_ok(),
            "a definitively rejected broadcast must release the receival \
             reservation for an immediate rebuild"
        );
    }

    /// The DEFAULT (`funding_path: None`) build must record and release its
    /// reservation against the unmixed BIP44 account too.
    ///
    /// `None` resolves to BIP44's own account-level path, so the release goes
    /// through the by-path lookup rather than the `AccountTypePreference` one —
    /// a different code path from every pre-existing release test, and the one
    /// the Kotlin default (`fundingPath = null`) will take on every ordinary
    /// send. If the two lookups disagreed about which account BIP44's path
    /// names, an ordinary send's reservation would never be released.
    #[tokio::test]
    async fn default_funding_reservation_is_held_and_released_on_bip44() {
        use crate::wallet::signed_payment_registry::SignedPaymentRegistry;

        let (wm, wallet_id, balance, signer) =
            funded_wallet_manager(StandardAccountType::BIP44Account).await;
        let core = core_wallet(wm, wallet_id, balance);

        let payment = core
            .finalize_signed_payment_from_funding_path(
                vec![(recipient(7), 1_000_000)],
                None,
                &signer,
                None,
            )
            .await
            .expect("0.01 DASH is fundable from the 0.1 DASH BIP44 account");
        assert!(
            matches!(payment.funding, FundingAccountRef::Path(_)),
            "the default build must record the RESOLVED BIP44 path, got {:?}",
            payment.funding
        );

        let registry: SignedPaymentRegistry<AlwaysRejectedBroadcaster> =
            SignedPaymentRegistry::new();
        let token = registry
            .register_funded_by(
                core.clone(),
                payment.transaction.clone(),
                payment.funding.clone(),
                Some(payment.reservation_height),
                payment.reservation_token,
            )
            .await;

        // Held: the fixture's single BIP44 UTXO is reserved.
        assert!(
            core.finalize_signed_payment_from_funding_path(
                vec![(recipient(8), 1_000_000)],
                None,
                &signer,
                None,
            )
            .await
            .is_err(),
            "the reserved BIP44 input must not be re-selectable while the token \
             is outstanding"
        );

        // Released: the by-path lookup found the same BIP44 account the
        // selector funded from.
        registry.release(token).await;
        assert!(
            core.finalize_signed_payment_from_funding_path(
                vec![(recipient(9), 1_000_000)],
                None,
                &signer,
                None,
            )
            .await
            .is_ok(),
            "releasing the token must return the BIP44 input to spendable"
        );
    }

    /// Input validation: empty outputs and zero-amount outputs are rejected
    /// before any wallet work.
    #[tokio::test]
    async fn rejects_empty_and_zero_outputs() {
        let (wm, wallet_id, balance, signer) =
            funded_wallet_manager(StandardAccountType::BIP44Account).await;
        let core = core_wallet(wm, wallet_id, balance);

        let empty = core.build_signed_payment(vec![], None, &signer, None).await;
        assert!(matches!(empty, Err(PlatformWalletError::TransactionBuild(_))));

        let zero = core
            .build_signed_payment(vec![(recipient(7), 0)], None, &signer, None)
            .await;
        assert!(matches!(zero, Err(PlatformWalletError::TransactionBuild(_))));
    }
}
