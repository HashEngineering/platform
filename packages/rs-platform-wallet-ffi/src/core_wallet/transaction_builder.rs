use crate::core_wallet_types::OutPointFFI;
use crate::error::*;
use crate::handle::{Handle, CORE_SIGNED_TRANSACTION_V2_STORAGE, PLATFORM_WALLET_STORAGE};
use crate::runtime::runtime;
use crate::types::{FFINetwork, Network};
use crate::{check_ptr, unwrap_option_or_return, unwrap_result_or_return};
use dashcore::blockdata::transaction::special_transaction::TransactionPayload;
use dashcore::hashes::Hash;
use dashcore::{Address as DashAddress, OutPoint, Transaction, TxOut, Txid};
use key_wallet::account::account_type::StandardAccountType;
use key_wallet::account::ManagedAccountCollection;
use key_wallet::managed_account::managed_account_trait::ManagedAccountTrait;
use key_wallet::managed_account::ManagedCoreFundsAccount;
use key_wallet::wallet::managed_wallet_info::coin_selection::SelectionStrategy;
use key_wallet::wallet::managed_wallet_info::fee::FeeRate;
use key_wallet::wallet::managed_wallet_info::transaction_builder::{
    TransactionBuilder, MAX_STANDARD_OP_RETURN_BYTES,
};
use key_wallet::wallet::managed_wallet_info::transaction_building::AccountTypePreference;
use key_wallet::wallet::managed_wallet_info::wallet_info_interface::WalletInfoInterface;
use rs_sdk_ffi::{MnemonicResolverCoreSigner, MnemonicResolverHandle};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::str::FromStr;

/// Opaque, C-compatible transaction builder. `inner` is a heap-boxed
/// key-wallet `TransactionBuilder`; `network` is the wallet network output
/// and change addresses are validated against.
///
/// NOT thread-safe: a single builder must be used from one thread at a time.
/// The setters mutate `*inner` in place (`take_builder`/`store_builder`)
/// without synchronization.
#[repr(C)]
pub struct FFITransactionBuilder {
    inner: *mut c_void,
    network: FFINetwork,
}

/// Broadcast it with `core_wallet_broadcast_transaction`, then release it
/// with `core_wallet_transaction_free`.
#[repr(C)]
pub struct FFICoreTransaction {
    tx_bytes: *mut u8,
    tx_len: usize,
    // Part of the C ABI (the Swift host reads `FFICoreTransaction.fee`); the
    // Rust side only writes it, so silence the never-read lint.
    #[allow(dead_code)]
    fee: u64,
}

/// Internal value behind the opaque V2 numeric handle. Keeping the originating
/// CoreWallet with the signed transaction lets `free` perform the same safe
/// reservation release as explicit abandon, even after the host discarded its
/// transient CoreWallet handle.
pub struct FFICoreSignedTransactionV2 {
    pub(crate) wallet: platform_wallet::CoreWallet<platform_wallet::broadcaster::SpvBroadcaster>,
    pub(crate) transaction: platform_wallet::SignedCoreTransaction,
}

impl FFICoreTransaction {
    pub(crate) fn bytes(&self) -> &[u8] {
        if self.tx_bytes.is_null() || self.tx_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.tx_bytes, self.tx_len) }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub enum CoreAccountTypeFFI {
    BIP44,
    BIP32,
    CoinJoin,
    /// Pool every spendable transparent source: BIP44 + BIP32 + all DashPay
    /// contact-receiving accounts (`platform_wallet::SEND_FUNDING_SOURCES`).
    /// Change returns to BIP44 (the first pooled source). CoinJoin stays out
    /// (separate privacy domain), as do a contact's watch-only external
    /// coins. The default selector for a plain send.
    AllSpendable,
}

impl CoreAccountTypeFFI {
    /// The single account family this selector names, or `None` for the
    /// pooled [`AllSpendable`](Self::AllSpendable) — used by APIs that address
    /// exactly one account (gap limits, per-account UTXO listing), which must
    /// reject the pooled selector with a typed parameter error.
    pub(crate) fn single_preference(self) -> Option<AccountTypePreference> {
        match self {
            CoreAccountTypeFFI::BIP44 => Some(AccountTypePreference::BIP44),
            CoreAccountTypeFFI::BIP32 => Some(AccountTypePreference::BIP32),
            CoreAccountTypeFFI::CoinJoin => Some(AccountTypePreference::CoinJoin),
            CoreAccountTypeFFI::AllSpendable => None,
        }
    }

    /// The funding sources this selector pools, in funding order — handed to
    /// [`CoreWallet::finalize_transaction`]'s multi-source API, whose first
    /// source supplies the change address. A single-family selector yields a
    /// one-element list, which keeps that API's strict one-account semantics.
    pub(crate) fn funding_sources(self) -> &'static [AccountTypePreference] {
        match self {
            CoreAccountTypeFFI::BIP44 => &[AccountTypePreference::BIP44],
            CoreAccountTypeFFI::BIP32 => &[AccountTypePreference::BIP32],
            CoreAccountTypeFFI::CoinJoin => &[AccountTypePreference::CoinJoin],
            CoreAccountTypeFFI::AllSpendable => &platform_wallet::SEND_FUNDING_SOURCES,
        }
    }
}

/// Atomically fund, reserve and sign a configured builder.
///
/// Unlike the deprecated `set_funding` + `build_signed` sequence, selection
/// and insertion into the account ReservationSet cannot interleave with a
/// competing finalizer. The wallet-manager lock is dropped before the host
/// mnemonic resolver is invoked. This function consumes `builder` on every
/// path after its pointer is accepted.
///
/// On success `out_transaction_handle` receives an opaque V2 handle. Consume
/// it with `core_wallet_broadcast_signed_transaction_v2` or
/// `core_wallet_abandon_signed_transaction_v2`.
///
/// If the host removes (or re-creates) this wallet while the external signer is
/// running, no handle is published: the build's reservation is reconciled and
/// this returns `NotFound` (98), the same code the deferred-token sibling
/// `core_wallet_signed_payment_finalize` uses for that case.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn core_wallet_tx_builder_finalize(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
    core_signer_handle: *mut MnemonicResolverHandle,
    out_transaction_handle: *mut Handle,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    check_ptr!(core_signer_handle);
    check_ptr!(out_transaction_handle);
    *out_transaction_handle = 0;

    let ffi = Box::from_raw(builder);
    let inner = *Box::from_raw(ffi.inner as *mut TransactionBuilder);
    let wallet = unwrap_option_or_return!(PLATFORM_WALLET_STORAGE.with_item(wallet, |w| w.clone()));

    let builder_network: Network = ffi.network.into();
    if builder_network != wallet.network() {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "builder network does not match wallet network".to_string(),
        );
    }

    let signer =
        MnemonicResolverCoreSigner::new(core_signer_handle, wallet.wallet_id(), wallet.network());
    let finalized = runtime().block_on(wallet.core().finalize_transaction(
        inner,
        account_type.funding_sources(),
        account_index,
        &signer,
    ));
    let finalized = unwrap_result_or_return!(finalized);

    // Publishing the V2 handle is gated exactly like the deferred-token sibling
    // below (`core_wallet_signed_payment_finalize`). `finalize_transaction` drops
    // the wallet-manager write lock before awaiting the (external, possibly slow)
    // signer, so the host can have removed this wallet while we were signing —
    // and that removal's V2-handle sweep has then ALREADY run. Inserting now
    // would publish a live handle for a removed generation that no later sweep
    // catches, and `core_wallet_broadcast_signed_transaction_v2` would happily
    // push it to the network: its `is_same_generation` check compares two
    // handles, and a removed generation matches itself (`dashpay/platform#4185`).
    //
    // Hold THIS generation's lifecycle gate across BOTH the liveness check and
    // the insert, so a teardown cannot interleave between them. Acquired AFTER
    // the signer await, never around it: holding it across an open signing prompt
    // would stall this wallet's teardown for as long as the user takes, and the
    // check makes that unnecessary.
    let (_lifecycle, wallet_is_live) = runtime().block_on(async {
        let gate = wallet.core().generation_payment_guard().await;
        let live = wallet.core().is_current_generation().await;
        (gate, live)
    });
    if !wallet_is_live {
        // No handle was published, so nothing would ever release this build's
        // reservation. Reconcile it here: the release is generation-bound, so on
        // a genuine removal it is a logged no-op (the `ReservationSet` died with
        // the generation), and on a re-create it correctly declines to touch the
        // new generation's inputs.
        runtime().block_on(wallet.core().abandon_transaction(&finalized));
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::NotFound,
            "wallet is no longer registered in the manager (removed or re-created while the \
             transaction was being signed); no transaction handle was published and its \
             reservation was reconciled"
                .to_string(),
        );
    }

    *out_transaction_handle =
        CORE_SIGNED_TRANSACTION_V2_STORAGE.insert(FFICoreSignedTransactionV2 {
            wallet: wallet.core().clone(),
            transaction: finalized,
        });
    PlatformWalletFFIResult::ok()
}

/// Value of the sole non-OP_RETURN output: what a broadcast of this
/// transaction actually pays out.
///
/// Returns 0 when there is no single such output. A multi-recipient build has
/// no one deliverable amount, and an OP_RETURN-only build pays no one — hosts
/// read the 0 as "not applicable" rather than "pays nothing", so the two cases
/// need not be told apart here.
///
/// Output ORDER is deliberately irrelevant: a MAYAChain deposit carries its
/// memo at VOUT1, while other layouts put the data carrier first.
fn sole_deliverable_value(outputs: &[TxOut]) -> u64 {
    let mut carriers = outputs.iter().filter(|out| !out.script_pubkey.is_op_return());
    match (carriers.next(), carriers.next()) {
        (Some(only), None) => only.value,
        _ => 0,
    }
}

/// Atomically fund, reserve, and sign a configured builder for DEFERRED
/// (BIP70/BIP270) submission, then register the built transaction — holding its
/// UTXO reservation — in one native operation.
///
/// This is the deferred counterpart to `core_wallet_tx_builder_finalize`: it
/// runs the same atomic `finalize_transaction`, where selection and insertion
/// into the account `ReservationSet` commit as a single unit under the
/// wallet-manager lock (signing happens after the lock is dropped). Routing the
/// deferred build through it closes the double-selection window that the
/// deprecated `set_funding` + `build_signed` + `register` sequence reopened once
/// the Kotlin per-wallet send mutex was removed: two concurrent deferred builds,
/// or a deferred build racing an immediate send, can no longer select the same
/// UTXO. Consumes `builder` on every path after its pointer is accepted.
///
/// Writes `out_token` (the reservation token for a later
/// `core_wallet_signed_payment_broadcast` / `core_wallet_signed_payment_release`),
/// `out_fee` (the build's fee in duffs), `out_txid` (a heap C string freed with
/// `core_wallet_free_address`), and `out_tx` (an owned `FFICoreTransaction`
/// carrying the consensus-serialized bytes, freed with
/// `core_wallet_transaction_free`). `out_bytes_ptr`/`out_bytes_len` borrow
/// `out_tx`'s buffer — copy them out before freeing `out_tx`.
///
/// Also writes `out_deliverable_duffs`: the value of the sole non-OP_RETURN
/// output of the REGISTERED transaction — what a later broadcast actually
/// pays out. Hosts need it for a drain (`SelectionStrategy::All`), where the
/// engine, not the caller, sets that output to `total inputs - fee`; reading it
/// from the registered transaction here keeps a quote and its payment from
/// disagreeing. Writes 0 when there is no single such output (multi-recipient,
/// or an OP_RETURN-only build) — "not applicable", not "pays nothing".
///
/// This is the CURRENT entry point. `core_wallet_signed_payment_finalize` is
/// the pre-existing eleven-argument symbol, kept so already-compiled callers
/// keep linking; it forwards here and discards the amount.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `wallet` a valid
/// platform-wallet handle; `core_signer_handle` a valid resolver handle; every
/// out-pointer must be writable. `out_tx` must point at writable storage for one
/// `FFICoreTransaction` (typically zeroed).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn core_wallet_signed_payment_finalize_with_deliverable(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
    core_signer_handle: *mut MnemonicResolverHandle,
    out_token: *mut u64,
    out_fee: *mut u64,
    out_txid: *mut *mut c_char,
    out_tx: *mut FFICoreTransaction,
    out_bytes_ptr: *mut *const u8,
    out_bytes_len: *mut usize,
    out_deliverable_duffs: *mut u64,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    check_ptr!(core_signer_handle);
    check_ptr!(out_token);
    check_ptr!(out_fee);
    check_ptr!(out_txid);
    check_ptr!(out_tx);
    check_ptr!(out_bytes_ptr);
    check_ptr!(out_bytes_len);
    check_ptr!(out_deliverable_duffs);
    *out_token = 0;
    *out_deliverable_duffs = 0;

    // `finalize_transaction` consumes the builder: reclaim both heap boxes up
    // front so they are freed on every return path below.
    let ffi = Box::from_raw(builder);
    let inner = *Box::from_raw(ffi.inner as *mut TransactionBuilder);

    let wallet = unwrap_option_or_return!(PLATFORM_WALLET_STORAGE.with_item(wallet, |w| w.clone()));

    let builder_network: Network = ffi.network.into();
    if builder_network != wallet.network() {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "builder network does not match wallet network".to_string(),
        );
    }

    let signer =
        MnemonicResolverCoreSigner::new(core_signer_handle, wallet.wallet_id(), wallet.network());

    // Atomic select + reserve + sign in one wallet-manager critical section.
    let finalized = runtime().block_on(wallet.core().finalize_transaction(
        inner,
        account_type.funding_sources(),
        account_index,
        &signer,
    ));
    let finalized = unwrap_result_or_return!(finalized);

    // `finalize_transaction` drops the wallet-manager write lock before awaiting
    // the (external, possibly slow) signer, so the host can have removed this
    // wallet while we were signing — and that removal's registry sweep has then
    // ALREADY run. Registering now would insert a live token for a removed
    // generation, which no later sweep would catch, defeating the teardown
    // invariant that dropping tokens makes stale handles inert
    // (`dashpay/platform#4185`).
    //
    // Take THIS wallet generation's lifecycle gate (shared — concurrent payments
    // are unaffected) and hold it across BOTH the liveness check and the
    // synchronous `register`, so a teardown cannot interleave between them.
    // Deliberately acquired AFTER the signer await rather than around it: holding
    // it across an open signing prompt would stall this wallet's teardown for as
    // long as the user takes, and the check below makes that unnecessary.
    let (_lifecycle, wallet_is_live) = runtime().block_on(async {
        let gate = wallet.core().generation_payment_guard().await;
        let live = wallet.core().is_current_generation().await;
        (gate, live)
    });
    if !wallet_is_live {
        // Nothing was registered, so no token would ever release this build's
        // reservation. Reconcile it here: the release is generation-bound, so on
        // a genuine removal it is a logged no-op (the `ReservationSet` died with
        // the generation), and on a re-create it correctly declines to touch the
        // new generation's inputs.
        runtime().block_on(wallet.core().abandon_transaction(&finalized));
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::NotFound,
            "wallet is no longer registered in the manager (removed or re-created while the \
             payment was being signed); the payment was not registered and its reservation was \
             reconciled"
                .to_string(),
        );
    }

    let txid = finalized.transaction().txid();
    let fee = finalized.fee();

    // Do the one fallible marshalling step BEFORE the registry insert: that
    // insert mints a token and keeps the funding reservation held, so a later
    // failure would orphan the reservation with no token to release it. txid hex
    // never contains a NUL, but handle the impossible case anyway.
    let c_txid = match CString::new(txid.to_string()) {
        Ok(s) => s,
        Err(_) => {
            // Nothing registered yet — release the reservation finalize took.
            runtime().block_on(wallet.core().abandon_transaction(&finalized));
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorUtf8Conversion,
                "txid string contained an interior NUL".to_string(),
            );
        }
    };

    // The deliverable amount, taken from the transaction that will actually be
    // broadcast — not re-derived by the host from a copy of the bytes. Under a
    // drain the ENGINE sets this output (total inputs - fee), so the caller
    // never supplied it and has no other authoritative source; a host that
    // re-parsed its own byte array could quote a value the broadcast does not
    // pay. Defined only for a single-destination payment: exactly one output
    // that is not an OP_RETURN data carrier. Anything else reports 0, which the
    // host reads as "not applicable" rather than "pays nothing".
    let deliverable_duffs = sole_deliverable_value(&finalized.transaction().output);
    unsafe { *out_deliverable_duffs = deliverable_duffs };

    let serialized = dashcore::consensus::serialize(finalized.transaction());
    let len = serialized.len();

    // Register the reserved+signed tx for deferred submission. `finalize` already
    // committed the reservation; `register` CONSUMES the `SignedCoreTransaction`
    // ownership object (deriving its transaction, funding account, reservation
    // height, and owner-guard token internally) and binds the token to the wallet
    // whose `ReservationSet` holds the inputs. Because the object is consumed
    // exactly once, this finalize can yield at most one token — no second token
    // can ever name the same reservation (`dashpay/platform#4185`, blocker 1).
    //
    // `register` is SYNCHRONOUS: its reservation-owning insert runs inline with
    // no future that could be dropped before its first poll and silently strand
    // the consumed reservation (`dashpay/platform#4185`). It also validates that
    // this wallet is the exact generation `finalize` bound the payment to; that
    // always holds here (we register through the very wallet that finalized), but
    // on the impossible mismatch it hands the finalized payment back so we
    // release its reservation (owner-guarded) rather than leaking it.
    let token = match crate::core_wallet::signed_payment::SIGNED_PAYMENT_REGISTRY
        .register(wallet.core().clone(), finalized)
    {
        Ok(token) => token,
        Err(err) => {
            runtime().block_on(wallet.core().abandon_transaction(&err.signed));
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorReservationWalletMismatch,
                "deferred payment was finalized against a different wallet generation".to_string(),
            );
        }
    };

    *out_tx = FFICoreTransaction {
        tx_bytes: Box::into_raw(serialized.into_boxed_slice()) as *mut u8,
        tx_len: len,
        fee,
    };
    *out_token = token.as_u64();
    *out_fee = fee;
    *out_txid = c_txid.into_raw();
    // Borrowed view into the just-written `out_tx` buffer; the caller copies the
    // bytes out before freeing `out_tx` with `core_wallet_transaction_free`.
    *out_bytes_ptr = (*out_tx).tx_bytes as *const u8;
    *out_bytes_len = len;
    PlatformWalletFFIResult::ok()
}

impl CoreAccountTypeFFI {
    /// The `StandardAccountType` this maps to, or `None` for `CoinJoin`.
    ///
    /// A CoinJoin-funded build DOES end up with reserved UTXOs — `build_signed`
    /// (via `assemble_unsigned`) reserves the selected inputs regardless of
    /// account type, since `set_funding` attaches the shared `ReservationSet`
    /// for every variant. But `reservations.rs`'s release-on-rejection is
    /// defined only over `StandardAccountType` (BIP44/BIP32). Returning `None`
    /// here routes CoinJoin through the plain broadcast, so a rejected CoinJoin
    /// tx keeps its reservation until the TTL backstop. That is intentional: the
    /// only CoinJoin funding path is a sweep — a single sender spending each
    /// UTXO exactly once, with no concurrent build or retry to race — so there
    /// is nothing to reconcile in practice.
    ///
    /// `AllSpendable` is likewise `None`, but for a different reason: a pooled
    /// build reserves inputs across SEVERAL accounts, so no single
    /// `StandardAccountType` can name its reservation. It is unreachable here
    /// by construction — the only build API this broadcast pairs with
    /// (`core_wallet_tx_builder_build_signed`) rejects the pooled selector with
    /// a typed parameter error, so no transaction reaches this function having
    /// been funded that way. A pooled build goes through
    /// `core_wallet_tx_builder_finalize`, whose handle carries the concrete
    /// `funding_accounts` that `broadcast_payment_releasing_reservation`
    /// releases on every contributing account.
    pub(crate) fn as_standard_account_type(&self) -> Option<StandardAccountType> {
        match self {
            CoreAccountTypeFFI::BIP44 => Some(StandardAccountType::BIP44Account),
            CoreAccountTypeFFI::BIP32 => Some(StandardAccountType::BIP32Account),
            CoreAccountTypeFFI::CoinJoin | CoreAccountTypeFFI::AllSpendable => None,
        }
    }
}

/// The pre-existing ELEVEN-argument finalize, preserved byte-for-byte in its
/// C signature. Forwards to
/// [`core_wallet_signed_payment_finalize_with_deliverable`] and discards the
/// deliverable amount; behaviour is otherwise identical.
///
/// Kept because this symbol is exported across a BINARY boundary: the Swift SDK
/// consumes `DashSDKFFI.xcframework` as a `binaryTarget`, so a host's compiled
/// Swift and this library are built and shipped separately and can meet at
/// different versions. Adding the twelfth out-parameter to this symbol in place
/// would make the callee write eight bytes through a pointer an eleven-argument
/// caller never passed — reading whatever occupied that argument slot and
/// treating it as an address. That corrupts silently rather than failing, so the
/// old shape stays, and callers that want the amount move to the new symbol.
///
/// Do not "simplify" this away by deleting it and updating the in-tree callers:
/// the callers that matter here are already-compiled binaries, which no
/// source-tree edit can reach.
///
/// # Safety
/// Identical to [`core_wallet_signed_payment_finalize_with_deliverable`], minus
/// `out_deliverable_duffs` (supplied internally).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn core_wallet_signed_payment_finalize(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
    core_signer_handle: *mut MnemonicResolverHandle,
    out_token: *mut u64,
    out_fee: *mut u64,
    out_txid: *mut *mut c_char,
    out_tx: *mut FFICoreTransaction,
    out_bytes_ptr: *mut *const u8,
    out_bytes_len: *mut usize,
) -> PlatformWalletFFIResult {
    // A real local, never null: the callee null-checks every out-pointer and
    // would reject the call outright.
    let mut discarded_deliverable_duffs: u64 = 0;
    core_wallet_signed_payment_finalize_with_deliverable(
        builder,
        wallet,
        account_type,
        account_index,
        core_signer_handle,
        out_token,
        out_fee,
        out_txid,
        out_tx,
        out_bytes_ptr,
        out_bytes_len,
        &mut discarded_deliverable_duffs,
    )
}

#[repr(C)]
pub enum CoreSelectionStrategyFFI {
    SmallestFirst,
    LargestFirst,
    BranchAndBound,
    OptimalConsolidation,
    Random,
    All,
}

impl From<CoreSelectionStrategyFFI> for SelectionStrategy {
    fn from(value: CoreSelectionStrategyFFI) -> Self {
        match value {
            CoreSelectionStrategyFFI::SmallestFirst => SelectionStrategy::SmallestFirst,
            CoreSelectionStrategyFFI::LargestFirst => SelectionStrategy::LargestFirst,
            CoreSelectionStrategyFFI::BranchAndBound => SelectionStrategy::BranchAndBound,
            CoreSelectionStrategyFFI::OptimalConsolidation => {
                SelectionStrategy::OptimalConsolidation
            }
            CoreSelectionStrategyFFI::Random => SelectionStrategy::Random,
            CoreSelectionStrategyFFI::All => SelectionStrategy::All,
        }
    }
}

fn managed_account(
    accounts: &ManagedAccountCollection,
    source: AccountTypePreference,
    account_index: u32,
) -> Option<&ManagedCoreFundsAccount> {
    source
        .account_type(account_index)
        .and_then(|at| accounts.funds_account(&at))
}

/// Mutable twin of [`managed_account`]. Delegates the family→account mapping
/// to key-wallet exactly as the shared reader does, so a DashPay preference —
/// which names a SET of accounts rather than one, and whose `account_type`
/// therefore yields `None` — resolves to no single account here instead of
/// needing an arm per new variant.
fn managed_account_mut(
    accounts: &mut ManagedAccountCollection,
    source: AccountTypePreference,
    account_index: u32,
) -> Option<&mut ManagedCoreFundsAccount> {
    source
        .account_type(account_index)
        .and_then(|at| accounts.funds_account_mut(&at))
}

impl FFITransactionBuilder {
    /// The inner builder taken out by value, leaving an empty one in its
    /// place. Pair with [`FFITransactionBuilder::store_builder`] to apply a
    /// fluent (`self -> Self`) method.
    ///
    /// # Safety
    /// `self.inner` must point at a live `TransactionBuilder`
    unsafe fn take_builder(&self) -> TransactionBuilder {
        std::mem::take(&mut *(self.inner as *mut TransactionBuilder))
    }

    /// Store `builder` back into the inner slot.
    ///
    /// # Safety
    /// `self.inner` must point at a live `TransactionBuilder`
    unsafe fn store_builder(&self, builder: TransactionBuilder) {
        *(self.inner as *mut TransactionBuilder) = builder;
    }
}

/// Create a new transaction builder for `network`. Free with
/// `core_wallet_tx_builder_destroy` (or `core_wallet_tx_builder_build_signed`,
/// which consumes it).
///
/// # Safety
/// The returned pointer is owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_new(
    network: FFINetwork,
) -> *mut FFITransactionBuilder {
    let inner = Box::into_raw(Box::new(TransactionBuilder::new())) as *mut c_void;
    Box::into_raw(Box::new(FFITransactionBuilder { inner, network }))
}

/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `address` a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_add_output(
    builder: *mut FFITransactionBuilder,
    address: *const c_char,
    amount: u64,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    check_ptr!(address);

    let addr_str = unwrap_result_or_return!(std::ffi::CStr::from_ptr(address).to_str());
    let network: Network = (*builder).network.into();
    let parsed = unwrap_result_or_return!(DashAddress::from_str(addr_str));
    let address = match parsed.require_network(network) {
        Ok(a) => a,
        Err(e) => {
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorInvalidParameter,
                format!("output address network mismatch: {e}"),
            );
        }
    };

    let b = (*builder).take_builder();
    let b = b.add_output(&address, amount);
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// Add a zero-value OP_RETURN output carrying `data`.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `data` must reference a
/// readable buffer of `data_len` bytes when `data_len > 0`.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_add_op_return(
    builder: *mut FFITransactionBuilder,
    data: *const u8,
    data_len: usize,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    if data_len > 0 {
        check_ptr!(data);
    }

    let bytes = if data_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data, data_len)
    };

    // `add_op_return` takes the builder by value, so a rejected payload drops it and leaves
    // `take_builder`'s `mem::take` default behind — silently discarding outputs and options
    // the caller already configured. Reject an over-long payload *before* taking the builder
    // so the slot keeps its real state. `add_op_return` re-checks; this is the same policy
    // constant, not a second opinion.
    if data_len > MAX_STANDARD_OP_RETURN_BYTES {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            format!(
                "OP_RETURN payload too large: {data_len} bytes (max {MAX_STANDARD_OP_RETURN_BYTES})"
            ),
        );
    }

    let b = (*builder).take_builder();
    let b = match b.add_op_return(bytes) {
        Ok(b) => b,
        Err(err) => {
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorWalletOperation,
                err.to_string(),
            );
        }
    };
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `address` a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_change_address(
    builder: *mut FFITransactionBuilder,
    address: *const c_char,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    check_ptr!(address);

    let addr_str = unwrap_result_or_return!(std::ffi::CStr::from_ptr(address).to_str());
    let network: Network = (*builder).network.into();
    let parsed = unwrap_result_or_return!(DashAddress::from_str(addr_str));
    let address = match parsed.require_network(network) {
        Ok(a) => a,
        Err(e) => {
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorInvalidParameter,
                format!("change address network mismatch: {e}"),
            );
        }
    };

    let b = (*builder).take_builder();
    let b = b.set_change_address(address);
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// Preserve outputs in the order they were added instead of applying BIP-69 sorting.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_preserve_output_order(
    builder: *mut FFITransactionBuilder,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let b = (*builder).take_builder();
    let b = b.preserve_output_order();
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// Route change to the address of the first selected input (VIN0).
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_change_to_first_input(
    builder: *mut FFITransactionBuilder,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let b = (*builder).take_builder();
    let b = b.change_to_first_input();
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// # Safety
/// `builder` must be a valid, non-destroyed pointer.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_fee_rate(
    builder: *mut FFITransactionBuilder,
    sat_per_kb: u64,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let b = (*builder).take_builder();
    let b = b.set_fee_rate(FeeRate::new(sat_per_kb));
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// # Safety
/// `builder` must be a valid, non-destroyed pointer.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_selection_strategy(
    builder: *mut FFITransactionBuilder,
    strategy: CoreSelectionStrategyFFI,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let b = (*builder).take_builder();
    let b = b.set_selection_strategy(strategy.into());
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// Set the block height coin selection treats as the chain tip (used for
/// coinbase maturity and locktime).
///
/// This value is advisory: `core_wallet_tx_builder_set_funding` and
/// `core_wallet_tx_builder_build_signed` both override it with the wallet's
/// last processed height when they run, so the wallet height always wins for
/// the funded/signed build. Use this only when building without a wallet.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_current_height(
    builder: *mut FFITransactionBuilder,
    height: u32,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let b = (*builder).take_builder();
    let b = b.set_current_height(height);
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// `payload_bytes` is a bincode-encoded `TransactionPayload`.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `payload_bytes` a readable buffer of
/// `payload_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_special_payload(
    builder: *mut FFITransactionBuilder,
    payload_bytes: *const u8,
    payload_len: usize,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    check_ptr!(payload_bytes);

    let bytes = std::slice::from_raw_parts(payload_bytes, payload_len);
    let payload: TransactionPayload =
        match bincode::decode_from_slice(bytes, bincode::config::standard()) {
            Ok((p, consumed)) => {
                if consumed != payload_len {
                    return PlatformWalletFFIResult::err(
                        PlatformWalletFFIResultCode::ErrorDeserialization,
                        format!(
                        "trailing bytes after payload: decoded {consumed} of {payload_len} bytes"
                    ),
                    );
                }
                p
            }
            Err(e) => {
                return PlatformWalletFFIResult::err(
                    PlatformWalletFFIResultCode::ErrorDeserialization,
                    format!("invalid special payload: {e}"),
                );
            }
        };

    let b = (*builder).take_builder();
    let b = b.set_special_payload(payload);
    (*builder).store_builder(b);

    PlatformWalletFFIResult::ok()
}

/// Fund the builder from the wallet account, setting inputs and change.
///
/// # Concurrency limitation (known, intentionally not fixed here)
/// key-wallet's `set_funding` filters out UTXOs already recorded in the
/// account's shared `ReservationSet`, but the reservation for *this* build is
/// only taken at build time (`assemble_unsigned` inside `build_signed`).
/// Because the FFI splits `set_funding` and `build_signed` across the C ABI —
/// the wallet lock cannot be held across the boundary — two concurrent builds
/// on the SAME account can both pass `set_funding` before either reserves and
/// select the same UTXO, producing a double-spend at broadcast. Single-threaded
/// callers (the SDK's send flow) are unaffected; concurrent same-account sends
/// must serialize at the call site.
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `wallet` a valid
/// platform-wallet handle.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_set_funding(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let wallet = unwrap_option_or_return!(PLATFORM_WALLET_STORAGE.with_item(wallet, |w| w.clone()));

    // Reject a builder created for a different network than the wallet.
    let builder_network: Network = (*builder).network.into();
    if builder_network != wallet.network() {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "builder network does not match wallet network".to_string(),
        );
    }

    let wallet_id = wallet.wallet_id();
    let Some(source) = account_type.single_preference() else {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "AllSpendable pools multiple accounts; this API addresses exactly one".to_string(),
        );
    };

    let result = runtime().block_on(async {
        let mut wm = wallet.wallet_manager().write().await;
        let (w, info) = wm
            .get_wallet_and_info_mut(&wallet_id)
            .ok_or_else(|| "wallet not found".to_string())?;

        // Resolve the xpub-bearing account through key-wallet's own
        // family→account mapping, the same way `managed_account_mut` resolves
        // the managed twin. `source` came from `single_preference`, so it is
        // always a single-account family here; delegating rather than matching
        // keeps this site from needing an arm per new DashPay variant.
        let account = source
            .account_type(account_index)
            .and_then(|at| w.accounts.account_of_type(at))
            .ok_or_else(|| format!("wallet account {source:?} #{account_index} not found"))?;

        let height = info.core_wallet.last_processed_height();

        let managed = managed_account_mut(&mut info.core_wallet.accounts, source, account_index)
            .ok_or_else(|| format!("managed account {source:?} #{account_index} not found"))?;

        // Resolution succeeded — only now consume the builder so a lookup
        // failure above can never leave it emptied.
        // `add_funding` is `set_funding` renamed by rust-dashcore#925 when
        // funding became ADDITIVE. The exported C symbol keeps its name so the
        // Kotlin/Swift bindings and the ABI are unchanged, but the semantics
        // this entry point inherits differ in two ways worth knowing:
        //
        // * Inputs the host seeded with `add_inputs` are no longer discarded.
        //   The old `set_funding` ASSIGNED `inputs`, silently dropping them;
        //   `add_funding` keeps them and filters any outpoint the builder
        //   already holds out of the candidate set, so it cannot be offered
        //   twice (rust-dashcore#931 — a duplicate prevout makes a transaction
        //   Core rejects).
        // * Calling this twice now POOLS both accounts instead of replacing the
        //   first. That is the intended primitive behind the pooled send; a host
        //   wanting single-account funding must call it once.
        let taken = (*builder).take_builder();
        let funded = taken
            .set_current_height(height)
            .add_funding(managed, account);
        (*builder).store_builder(funded);
        Ok::<_, String>(())
    });

    match result {
        Ok(()) => PlatformWalletFFIResult::ok(),
        Err(e) => PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorWalletOperation,
            format!("set_funding failed: {e}"),
        ),
    }
}

/// Add a caller-chosen subset of the account's UTXOs as inputs. `outpoints`
/// are selected from the account's own UTXO set (the same ones
/// `platform_wallet_account_utxos` returns). An outpoint not owned by the
/// account is an error
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `wallet` a valid platform-wallet handle;
/// `outpoints` a readable array of `outpoints_len` elements.
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_add_inputs_from_outpoints(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
    outpoints: *const OutPointFFI,
    outpoints_len: usize,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);

    let wallet = unwrap_option_or_return!(PLATFORM_WALLET_STORAGE.with_item(wallet, |w| w.clone()));

    // Reject a builder created for a different network than the wallet, matching
    // `set_funding` / `build_signed` so all three wallet-aware entry points fail
    // fast instead of mutating a foreign-network builder.
    let builder_network: Network = (*builder).network.into();
    if builder_network != wallet.network() {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "builder network does not match wallet network".to_string(),
        );
    }

    let wallet_id = wallet.wallet_id();
    let Some(source) = account_type.single_preference() else {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "AllSpendable pools multiple accounts; this API addresses exactly one".to_string(),
        );
    };

    let requested: Vec<OutPoint> = if outpoints_len == 0 {
        Vec::new()
    } else {
        check_ptr!(outpoints);
        std::slice::from_raw_parts(outpoints, outpoints_len)
            .iter()
            .map(|op| OutPoint {
                txid: Txid::from_byte_array(op.txid),
                vout: op.vout,
            })
            .collect()
    };

    let result = runtime().block_on(async {
        let wm = wallet.wallet_manager().read().await;
        let info = wm
            .get_wallet_info(&wallet_id)
            .ok_or_else(|| "wallet not found".to_string())?;

        let managed = managed_account(&info.core_wallet.accounts, source, account_index)
            .ok_or_else(|| format!("managed account {source:?} #{account_index} not found"))?;

        let mut selected = Vec::with_capacity(requested.len());
        for op in &requested {
            let utxo = managed
                .utxos
                .get(op)
                .cloned()
                .ok_or_else(|| format!("outpoint {}:{} not in account", op.txid, op.vout))?;
            selected.push(utxo);
        }

        // Validation succeeded — only now consume the builder.
        let taken = (*builder).take_builder();
        (*builder).store_builder(taken.add_inputs(selected));
        Ok::<_, String>(())
    });

    match result {
        Ok(()) => PlatformWalletFFIResult::ok(),
        Err(e) => PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorWalletOperation,
            format!("add_inputs_from_outpoints failed: {e}"),
        ),
    }
}

/// Build and sign, resolving signing paths from the wallet account. Returns
/// consensus-serialized signed bytes and the fee.
///
/// This function also frees the builder
///
/// # Safety
/// `builder` must be a valid, non-destroyed pointer; `wallet` a valid platform-wallet handle;
/// `core_signer_handle` a valid, non-destroyed resolver handle; `out_tx` a
/// writable pointer the caller later frees with `core_wallet_transaction_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn core_wallet_tx_builder_build_signed(
    builder: *mut FFITransactionBuilder,
    wallet: Handle,
    account_type: CoreAccountTypeFFI,
    account_index: u32,
    core_signer_handle: *mut MnemonicResolverHandle,
    out_tx: *mut FFICoreTransaction,
) -> PlatformWalletFFIResult {
    check_ptr!(builder);
    // `build` consumes the builder: reclaim both heap boxes up front so they
    // are freed on every return path below
    let ffi = Box::from_raw(builder);
    let inner = *Box::from_raw(ffi.inner as *mut TransactionBuilder);

    check_ptr!(core_signer_handle);
    check_ptr!(out_tx);

    let wallet = unwrap_option_or_return!(PLATFORM_WALLET_STORAGE.with_item(wallet, |w| w.clone()));

    // Backstop network check: reject a builder built for a different network
    // than the wallet, even when `set_funding` already validated it.
    let builder_network: Network = ffi.network.into();
    if builder_network != wallet.network() {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "builder network does not match wallet network".to_string(),
        );
    }

    let wallet_id = wallet.wallet_id();
    let Some(source) = account_type.single_preference() else {
        return PlatformWalletFFIResult::err(
            PlatformWalletFFIResultCode::ErrorInvalidParameter,
            "AllSpendable pools multiple accounts; this API addresses exactly one".to_string(),
        );
    };
    let signer = MnemonicResolverCoreSigner::new(core_signer_handle, wallet_id, wallet.network());

    let build = runtime().block_on(async {
        let wm = wallet.wallet_manager().read().await;
        let info = wm
            .get_wallet_info(&wallet_id)
            .ok_or_else(|| "wallet not found".to_string())?;

        let height = info.core_wallet.last_processed_height();

        let managed = managed_account(&info.core_wallet.accounts, source, account_index)
            .ok_or_else(|| format!("managed account {source:?} #{account_index} not found"))?;

        inner
            .set_current_height(height)
            .build_signed(&signer, |addr| managed.address_derivation_path(&addr))
            .await
            .map_err(|e| e.to_string())
    });

    let (tx, fee): (Transaction, u64) = match build {
        Ok(v) => v,
        Err(e) => {
            return PlatformWalletFFIResult::err(
                PlatformWalletFFIResultCode::ErrorWalletOperation,
                format!("transaction build failed: {e}"),
            );
        }
    };

    let serialized = dashcore::consensus::serialize(&tx);
    let len = serialized.len();

    *out_tx = FFICoreTransaction {
        tx_bytes: Box::into_raw(serialized.into_boxed_slice()) as *mut u8,
        tx_len: len,
        fee,
    };

    PlatformWalletFFIResult::ok()
}

/// Destroy a transaction builder created by `core_wallet_tx_builder_new`.
///
/// # Safety
/// `builder` must not have already been destroyed or built (or null).
#[no_mangle]
pub unsafe extern "C" fn core_wallet_tx_builder_destroy(builder: *mut FFITransactionBuilder) {
    if builder.is_null() {
        return;
    }

    let b = Box::from_raw(builder);
    let _ = Box::from_raw(b.inner as *mut TransactionBuilder);
}

/// Free a transaction returned by `core_wallet_tx_builder_build_signed`.
/// Idempotent: the fields are nulled, so a second call is a no-op.
///
/// # Safety
/// `tx` must be a valid pointer to an `FFICoreTransaction` from
/// `core_wallet_tx_builder_build_signed` (or null).
#[no_mangle]
pub unsafe extern "C" fn core_wallet_transaction_free(tx: *mut FFICoreTransaction) {
    if tx.is_null() {
        return;
    }

    let tx = &mut *tx;
    if !tx.tx_bytes.is_null() && tx.tx_len > 0 {
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(tx.tx_bytes, tx.tx_len));
    }

    tx.tx_bytes = std::ptr::null_mut();
    tx.tx_len = 0;
}


#[cfg(test)]
mod tests {
    use super::sole_deliverable_value;
    use dashcore::blockdata::script::ScriptBuf;
    use dashcore::TxOut;

    /// A spendable output. The script only has to NOT be an OP_RETURN.
    fn destination(value: u64) -> TxOut {
        TxOut {
            value,
            script_pubkey: ScriptBuf::from(vec![0x76, 0xa9, 0x14]),
        }
    }

    fn op_return(payload: &[u8]) -> TxOut {
        let data = dashcore::script::PushBytesBuf::try_from(payload.to_vec())
            .expect("test payload is within push limits");
        TxOut {
            value: 0,
            script_pubkey: ScriptBuf::new_op_return(&data),
        }
    }

    #[test]
    fn a_lone_destination_is_the_deliverable_amount() {
        assert_eq!(sole_deliverable_value(&[destination(27_442_985)]), 27_442_985);
    }

    /// The MAYAChain shape: vault output plus a zero-value memo. The memo must
    /// not be mistaken for a second recipient, in EITHER order — Maya puts the
    /// memo at VOUT1, but nothing in the calculation may depend on that.
    #[test]
    fn a_data_carrier_beside_the_destination_is_ignored_in_both_orders() {
        let memo = op_return(b"=:MAYA.CACAO:maya1abc");
        assert_eq!(
            sole_deliverable_value(&[destination(27_442_985), memo.clone()]),
            27_442_985,
            "memo after the destination (the Maya layout)"
        );
        assert_eq!(
            sole_deliverable_value(&[memo, destination(27_442_985)]),
            27_442_985,
            "memo before the destination"
        );
    }

    /// Two recipients have no single deliverable amount. Reporting either one
    /// would let a host quote a number the payment does not pay.
    #[test]
    fn two_spendable_outputs_report_zero() {
        assert_eq!(sole_deliverable_value(&[destination(1_000), destination(2_000)]), 0);
    }

    #[test]
    fn two_spendable_outputs_report_zero_even_beside_a_data_carrier() {
        assert_eq!(
            sole_deliverable_value(&[destination(1_000), op_return(b"x"), destination(2_000)]),
            0
        );
    }

    /// An OP_RETURN-only build pays no one; so does an empty output set.
    #[test]
    fn a_transaction_with_no_spendable_output_reports_zero() {
        assert_eq!(sole_deliverable_value(&[op_return(b"data only")]), 0);
        assert_eq!(sole_deliverable_value(&[]), 0);
    }

    /// An asset lock's single output IS an OP_RETURN, so it reports 0 rather
    /// than its burn value. That is the intended reading: the credits go to an
    /// identity, not to a payee a host would quote.
    #[test]
    fn an_op_return_carrying_value_still_reports_zero() {
        let mut burn = op_return(b"credits");
        burn.value = 500_000;
        assert_eq!(sole_deliverable_value(&[burn]), 0);
    }
}
