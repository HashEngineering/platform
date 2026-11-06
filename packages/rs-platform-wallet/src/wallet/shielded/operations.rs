//! Shielded transaction operations (6 transition types), multi-account.
//!
//! Each operation is a free function taking the
//! (sdk, store, persister, wallet_id, keys, account, …) tuple
//! it needs explicitly. Phase 4d.3 lifted these out of
//! `impl<S> ShieldedWallet<S>` so per-wallet shielded state on
//! `PlatformWallet` can be just the keys map without a wrapper
//! struct.
//!
//! Spends never cross account boundaries — note selection reads
//! only the given account's unspent notes.
//!
//! The six transition types are:
//! - **Shield** (Type 15): transparent platform addresses → shielded pool
//! - **ShieldFromAssetLock** (Type 18): Core L1 asset lock → shielded pool
//! - **Unshield** (Type 17): shielded pool → transparent platform address
//! - **Transfer** (Type 16): shielded pool → shielded pool (private)
//! - **Withdraw** (Type 19): shielded pool → Core L1 address
//! - **IdentityCreateFromShieldedPool** (Type 20): shielded pool → a brand-new Platform identity
//!   funded by a fixed denomination leaving the pool (any excess re-enters as a change note)

use super::activity::{
    live_transfer_multi_row, ShieldedActivityKind, ShieldedActivityStatus, ShieldedDirection,
};
use super::activity_recorder::{
    build_pending_entry, changeset_for_entry, non_zero_memo, with_status, LiveEntryParams,
};
use super::keys::{AccountViewingKeys, OrchardKeySet};
use super::note_selection::{
    ensure_allowed_denomination, ensure_positive_amount, select_notes_for_denomination,
    select_notes_with_fee, ChangeRequirement, ShieldedFeeKind,
};
use super::store::{
    PendingRedrive, PrivateContactPayment, PrivatePaymentEntry, ShieldedNote, ShieldedStore,
    SubwalletId,
};
use crate::broadcast_outcome::{
    broadcast_tracking_delivery, broadcast_was_refused, carries_consensus_rejection,
    BroadcastDelivery,
};
use crate::changeset::{PlatformWalletChangeSet, ShieldedChangeSet};
use crate::error::{preserve_signer_key_unavailable_or, PlatformWalletError};
use crate::wallet::identity::network::PaymentStarted;
use crate::wallet::persister::WalletPersister;
use crate::wallet::platform_wallet::WalletId;
use sha2::{Digest, Sha256};

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use dash_sdk::dapi_grpc::platform::v0::ResponseMetadata;
use dash_sdk::platform::fetch_current_no_parameters::FetchCurrent;
use dash_sdk::platform::transition::broadcast::BroadcastStateTransition;
use dash_sdk::platform::transition::identity_create_from_shielded_pool::IdentityCreateFromShieldedPool;
use dash_sdk::platform::Fetch;
use dash_sdk::query_types::IdentityNonceFetcher;
use dpp::address_funds::{
    AddressFundsFeeStrategy, AddressFundsFeeStrategyStep, OrchardAddress, PlatformAddress,
};
use dpp::fee::Credits;
use dpp::identity::accessors::{IdentityGettersV0, IdentitySettersV0};
use dpp::identity::core_script::CoreScript;
use dpp::identity::identity_nonce::{
    IDENTITY_NONCE_VALUE_FILTER, IDENTITY_NONCE_VALUE_FILTER_MAX_BYTES,
    MAX_MISSING_IDENTITY_REVISIONS, MISSING_IDENTITY_REVISIONS_FILTER,
};
use dpp::identity::identity_public_key::accessors::v0::IdentityPublicKeyGettersV0;
use dpp::identity::signer::Signer;
use dpp::identity::{Identity, IdentityPublicKey};
use dpp::platform_value::BinaryData;
use dpp::prelude::{AddressNonce, Identifier, IdentityNonce};
use dpp::shielded::builder::{
    build_identity_create_from_shielded_pool_transition,
    build_identity_top_up_from_shielded_pool_transition,
    build_private_document_create_from_shielded_pool_transition,
    build_shield_from_identity_transition, build_shield_transition,
    build_shielded_transfer_transition, build_shielded_transfer_transition_multi,
    build_shielded_withdrawal_transition, build_unshield_transition, OrchardProver,
    ShieldedTransferOutput, SpendableNote,
};
use dpp::shielded::compute_minimum_shielded_fee;
use dpp::state_transition::proof_result::StateTransitionProofResult;
use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::identity_id_from_nullifiers;
use dpp::state_transition::public_key_in_creation::IdentityPublicKeyInCreation;
use dpp::state_transition::StateTransition;
use dpp::version::PlatformVersion;
use dpp::withdrawal::Pooling;
use dpp::ProtocolError;
use grovedb_commitment_tree::{Anchor, PaymentAddress};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

/// Number of Orchard actions in a `Shield` (Type 15) bundle.
///
/// `build_shield_transition` builds an *output-only* bundle with a single
/// output (`build_output_only_bundle`), configured as Orchard's
/// `BundleType::Transactional { flags: SPENDS_DISABLED, bundle_required: false }`.
/// For one output and zero spends, Orchard's `num_actions` is
/// `max(max(0, 1), MIN_ACTIONS) == max(1, 2) == 2`, so the serialized bundle
/// always carries exactly two actions. Consensus prices the flat shielded fee
/// `F = compute_minimum_shielded_fee(actions.len())` from the on-wire action
/// count, so the wallet's fee reservation must use the same count.
const SHIELD_NUM_ACTIONS: usize = 2;

/// Multiplier applied to the versioned minimum shield fee when sizing the
/// planner's input-0 reserve.
///
/// Execution deducts the ACTUAL fee — the GroveDB-metered storage/processing
/// of the note/nullifier writes plus `compute_shielded_verification_fee` —
/// from input 0's post-reallocation residue, and rejects the shield when the
/// residue can't cover it. `compute_minimum_shielded_fee` estimates that
/// actual fee with a flat per-action storage term the client cannot meter
/// itself, so the reserve keeps one extra fee of headroom for metering
/// variance. The reserve is NOT what satisfies the structure gate
/// (`Σ claims ≥ amount + fee`) — `reserve_shield_fee_on_input_0` loads the
/// claimed fee for that — so it needs no allowance beyond metering variance.
const SHIELD_FEE_RESERVE_MULTIPLIER: u64 = 2;

/// Versioned balance the shield planner keeps unclaimed on the
/// lexicographically first (fee-paying) input.
///
/// The preflight and the execution path both derive capacity from this one
/// value, so it directly sets three host-visible numbers: the viability
/// threshold an address must exceed to serve as input 0, the account's
/// `max_shieldable_credits`, and the residue a Max shield leaves transparent
/// (`reserve − actual fee`). Deriving it from the versioned fee formula keeps
/// all three tracking fee-constant bumps instead of freezing a magic number
/// that overstates the fee and understates capacity.
pub fn shield_fee_reserve_credits(
    platform_version: &PlatformVersion,
) -> Result<Credits, PlatformWalletError> {
    let fee = compute_minimum_shielded_fee(SHIELD_NUM_ACTIONS, platform_version)
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
    fee.checked_mul(SHIELD_FEE_RESERVE_MULTIPLIER)
        .ok_or_else(|| {
            PlatformWalletError::ShieldedBuildError("shield fee reserve overflows u64".to_string())
        })
}

/// Try to extract a structured `AddressesNotEnoughFundsError` from
/// a broadcast error so the shield path can format a diagnostic
/// that includes Platform's actual per-input view (nonce + balance)
/// rather than just the stringified message.
fn addresses_not_enough_funds(
    e: &dash_sdk::Error,
) -> Option<&dpp::consensus::state::address_funds::AddressesNotEnoughFundsError> {
    use dpp::consensus::state::state_error::StateError;
    use dpp::consensus::ConsensusError;
    use dpp::ProtocolError;

    let consensus: &ConsensusError = match e {
        dash_sdk::Error::Protocol(ProtocolError::ConsensusError(boxed)) => boxed.as_ref(),
        dash_sdk::Error::StateTransitionBroadcastError(s) => s.cause.as_ref()?,
        _ => return None,
    };
    match consensus {
        ConsensusError::StateError(StateError::AddressesNotEnoughFundsError(err)) => Some(err),
        _ => None,
    }
}

/// Try to extract the per-address `AddressNotEnoughFundsError` that the
/// pre-flight `fetch_inputs_with_nonce` hard balance check raises when a
/// single input is short. Mirrors [`addresses_not_enough_funds`] (the
/// plural broadcast-side variant) so the pre-broadcast failure can carry
/// the same structured `(address, balance, required)` info across the
/// FFI instead of collapsing to an opaque stringified error.
fn address_not_enough_funds(
    e: &dash_sdk::Error,
) -> Option<&dpp::consensus::state::address_funds::AddressNotEnoughFundsError> {
    use dpp::consensus::state::state_error::StateError;
    use dpp::consensus::ConsensusError;
    use dpp::ProtocolError;

    let consensus: &ConsensusError = match e {
        dash_sdk::Error::Protocol(ProtocolError::ConsensusError(boxed)) => boxed.as_ref(),
        dash_sdk::Error::StateTransitionBroadcastError(s) => s.cause.as_ref()?,
        _ => return None,
    };
    match consensus {
        ConsensusError::StateError(StateError::AddressNotEnoughFundsError(err)) => Some(err),
        _ => None,
    }
}

/// Promote the shield pre-broadcast hard balance check into the typed capacity
/// error the FFI and Swift layers recognize.
///
/// The values are Platform's live per-input view, not the cached planner
/// snapshot. Their `Display` rendering therefore preserves the actionable
/// available/required diagnostic while the typed variant lets the host refresh
/// preflight instead of retrying the stale amount unchanged.
fn map_shield_input_fetch_error(e: &dash_sdk::Error) -> PlatformWalletError {
    match address_not_enough_funds(e) {
        Some(short) => PlatformWalletError::PlatformShieldCapacityExceeded {
            available: short.balance(),
            required: short.required_balance(),
        },
        None => PlatformWalletError::ShieldedBuildError(format!("fetch input nonces: {e}")),
    }
}

/// Format a one-line `addresses_with_info` summary for diagnostics —
/// each entry rendered as `<bech32m_addr>=(nonce <n>, <c> credits)`,
/// matching what the wallet UI shows.
fn format_addresses_with_info(
    map: &std::collections::BTreeMap<
        dpp::address_funds::PlatformAddress,
        (dpp::prelude::AddressNonce, dpp::fee::Credits),
    >,
    network: key_wallet::Network,
) -> String {
    map.iter()
        .map(|(addr, (nonce, credits))| {
            format!(
                "{}=(nonce {nonce}, {credits} credits)",
                addr.to_bech32m_string(network)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Queue a shielded changeset on the persister if one is
/// attached. No-op if the changeset is empty or no persister
/// was supplied.
pub(super) fn queue_shielded_changeset(
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    cs: ShieldedChangeSet,
) {
    if cs.is_empty() {
        return;
    }
    let Some(persister) = persister else {
        return;
    };
    let full = PlatformWalletChangeSet {
        shielded: Some(cs),
        ..Default::default()
    };
    if let Err(e) = persister.store(full) {
        tracing::warn!(
            wallet_id = %hex::encode(wallet_id),
            error = %e,
            "Failed to queue shielded changeset"
        );
    }
}

/// Write an activity entry to the in-memory store, then queue it to the
/// host persister. For callers outside this module (the Type 18
/// orchestrator in `fund_from_asset_lock.rs`). Upserts by `entry.id`.
///
/// The in-memory `save_activity` MUST happen before the persister queue so
/// the same-session scan deriver's dedupe (which reads the in-memory store)
/// sees this live row and never re-derives a coarser entry over it. A store
/// write failure is logged but does not fail the op — the persister still
/// gets the entry, and the next sync reconciles the in-memory side.
pub(super) async fn queue_shielded_activity<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    entry: super::activity::ShieldedActivityEntry,
) {
    if let Err(e) = store.write().await.save_activity(id, &entry) {
        warn!(
            entry_id = %hex::encode(entry.id),
            error = %e,
            "live activity entry: in-memory save_activity failed; persister still queued"
        );
    }
    queue_shielded_changeset(persister, wallet_id, changeset_for_entry(id, entry));
}

/// Extract the serialized Orchard actions from a built shielded
/// `StateTransition`, for live activity-entry recording.
///
/// Returns `&[]` for any non-shielded variant (none reach the recorder).
/// Each shielded variant's `actions()` comes from its own accessor /
/// methods trait, so the relevant traits are imported locally.
fn shielded_actions(st: &StateTransition) -> &[dpp::shielded::SerializedAction] {
    // `actions()` is on each type's accessor trait; Shield exposes `actions`
    // only as a public field on its V0 struct, so match down to that
    // variant for it.
    use dpp::state_transition::shield_from_asset_lock_transition::accessors::ShieldFromAssetLockTransitionAccessorsV0;
    use dpp::state_transition::shield_transition::ShieldTransition;
    use dpp::state_transition::shielded_transfer_transition::accessors::ShieldedTransferTransitionAccessorsV0;
    use dpp::state_transition::shielded_withdrawal_transition::accessors::ShieldedWithdrawalTransitionAccessorsV0;
    use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::accessors::IdentityCreateFromShieldedPoolTransitionAccessorsV0;
    use dpp::state_transition::identity_top_up_from_shielded_pool_transition::accessors::IdentityTopUpFromShieldedPoolTransitionAccessorsV0;
    use dpp::state_transition::shield_from_identity_transition::accessors::ShieldFromIdentityTransitionAccessorsV0;
    use dpp::state_transition::unshield_transition::accessors::UnshieldTransitionAccessorsV0;
    use dpp::state_transition::private_document_create_from_shielded_pool_transition::accessors::PrivateDocumentCreateFromShieldedPoolTransitionAccessorsV0;

    match st {
        StateTransition::Shield(ShieldTransition::V0(v0)) => &v0.actions,
        StateTransition::ShieldedTransfer(t) => t.actions(),
        StateTransition::Unshield(t) => t.actions(),
        StateTransition::ShieldFromAssetLock(t) => t.actions(),
        StateTransition::ShieldedWithdrawal(t) => t.actions(),
        StateTransition::IdentityCreateFromShieldedPool(t) => t.actions(),
        StateTransition::ShieldFromIdentity(t) => t.actions(),
        StateTransition::IdentityTopUpFromShieldedPool(t) => t.actions(),
        StateTransition::PrivateDocumentCreateFromShieldedPool(t) => t.actions(),
        _ => &[],
    }
}

/// Record a live activity entry built from `params` for `id`, writing it
/// to the in-memory `store` and then shipping it to the host persister.
/// Returns the recorded entry so the caller can flip its status later (the
/// status flip re-records by the same `entry.id`, an upsert).
///
/// The in-memory `save_activity` MUST land before the persister queue so
/// the same-session scan deriver's dedupe (which reads the IN-MEMORY store
/// — see `coordinator::derive_activity_into_changeset`) sees this live row
/// and never re-derives a coarser entry that `save_activity` would upsert
/// over it. See [`queue_shielded_activity`].
///
/// Returns `None` (and queues nothing) when the bundle exposes no
/// wallet-visible output cmx — see [`build_pending_entry`].
async fn record_pending_activity<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    keys: &AccountViewingKeys,
    params: LiveEntryParams<'_>,
) -> Option<super::activity::ShieldedActivityEntry> {
    let kind = params.kind.clone();
    let Some(entry) = build_pending_entry(keys, params) else {
        // Should be unreachable for bundles our own builders produced
        // (they always carry at least one wallet-visible output) — but a
        // silent skip here means the operation leaves no history, so
        // make it loud.
        warn!(
            ?kind,
            "live activity entry skipped: no wallet-visible output cmx recovered from the bundle"
        );
        return None;
    };
    info!(
        ?kind,
        entry_id = %hex::encode(entry.id),
        cmxs = entry.note_cmxs.len(),
        "live activity entry recorded (pending)"
    );
    queue_shielded_activity(store, persister, wallet_id, id, entry.clone()).await;
    Some(entry)
}

/// Re-record `entry` with a flipped status (and optional confirmed
/// height) for `id`. Upserts by `entry.id` in the in-memory store and at
/// the persister, so a `Pending` row becomes `Confirmed` / `Failed` in
/// place. No-op when `entry` is `None` (nothing was recorded for this
/// operation).
///
/// The flip is applied to the CURRENT stored row (read under the same
/// write lock as the save), not the captured pre-broadcast entry: a
/// concurrent scan pass can upgrade the row to `Confirmed` at a real
/// height between broadcast and the result-wait, and rewriting the stale
/// capture would erase that scan-learned height. A row the scan already
/// confirmed WITH a height is chain truth and is left untouched
/// entirely — nothing the result-wait knows can improve on it.
async fn record_activity_status<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    entry: &Option<super::activity::ShieldedActivityEntry>,
    status: ShieldedActivityStatus,
    block_height: Option<u64>,
) {
    let Some(entry) = entry else {
        return;
    };
    let next = {
        let mut store = store.write().await;
        // A read FAILURE is not "no row": falling back to the captured
        // entry on Err would queue exactly the stale overwrite this
        // function exists to prevent, precisely when the invariant
        // couldn't be checked. Skip the flip instead — the scan
        // confirmation path reconciles the row's status later.
        let stored = match store.get_activity_by_entry_id(id, &entry.id) {
            Ok(stored) => stored,
            Err(e) => {
                warn!(
                    entry_id = %hex::encode(entry.id),
                    error = %e,
                    "live activity status flip: get_activity_by_entry_id failed; \
                     skipping flip to avoid clobbering a richer row"
                );
                return;
            }
        };
        let base = stored.as_ref().unwrap_or(entry);
        if base.status == ShieldedActivityStatus::Confirmed && base.block_height.is_some() {
            return;
        }
        // `with_status` keeps `base.block_height` when `block_height` is
        // `None`, so a height the scan populated survives the flip.
        let next = with_status(base, status, block_height);
        if let Err(e) = store.save_activity(id, &next) {
            warn!(
                entry_id = %hex::encode(next.id),
                error = %e,
                "live activity status flip: in-memory save_activity failed; persister still queued"
            );
        }
        next
    };
    queue_shielded_changeset(persister, wallet_id, changeset_for_entry(id, next));
}

/// Flip the activity row identified by `entry_id` (if present) to
/// `status`, reusing [`record_activity_status`]'s semantics — it re-reads
/// the CURRENT stored row and leaves a scan-`Confirmed`-with-height row
/// untouched. Used by the sync reconcile, which knows only the reservation's
/// stored `activity_id`, not the whole entry. No-op if no such row exists.
pub(super) async fn record_activity_status_by_id<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    entry_id: &[u8; 32],
    status: ShieldedActivityStatus,
) {
    let entry = match store.read().await.get_activity_by_entry_id(id, entry_id) {
        Ok(entry) => entry,
        Err(e) => {
            warn!(
                entry_id = %hex::encode(entry_id),
                error = %e,
                "activity status flip by id: lookup failed; skipping"
            );
            return;
        }
    };
    record_activity_status(store, persister, wallet_id, id, &entry, status, None).await;
}

// -------------------------------------------------------------------------
// Shield: platform addresses -> shielded pool (Type 15)
// -------------------------------------------------------------------------

/// Add the flat shielded fee `fee` to the smallest-key input's claim.
///
/// The Shield fee strategy is `DeductFromInput(0)`, where "input 0" is the
/// BTreeMap-smallest address. Consensus requires the input claims to sum to at
/// least `amount + fee`; the caller's selection claims exactly `amount` and
/// reserves the fee headroom as *unclaimed* balance on input 0, so loading the
/// fee onto that same input both satisfies the `Σ inputs >= amount + fee`
/// structure check and keeps the fee-payer aligned with the fee strategy.
///
/// Errors if `inputs` is empty (no input to carry the fee) or the addition
/// overflows.
fn reserve_shield_fee_on_input_0(
    mut inputs: BTreeMap<PlatformAddress, Credits>,
    fee: Credits,
) -> Result<BTreeMap<PlatformAddress, Credits>, PlatformWalletError> {
    let Some((&input_0_addr, _)) = inputs.iter().next() else {
        return Err(PlatformWalletError::ShieldedBuildError(
            "shield has no inputs to carry the shielded fee".to_string(),
        ));
    };
    let claim = inputs
        .get_mut(&input_0_addr)
        .expect("input_0_addr was just read from the map");
    *claim = claim.checked_add(fee).ok_or_else(|| {
        PlatformWalletError::ShieldedBuildError(
            "input 0 claim + shielded fee overflows u64".to_string(),
        )
    })?;
    Ok(inputs)
}

/// The resolved Orchard output and live-activity classification for a
/// shield — see [`resolve_shield_recipient`].
#[derive(Debug)]
struct ShieldRecipient {
    /// The address the note is built for.
    address: OrchardAddress,
    /// Raw 43-byte recipient for the activity row (`Some` only for a
    /// third-party recipient).
    counterparty: Option<Vec<u8>>,
    kind: ShieldedActivityKind,
    direction: ShieldedDirection,
}

/// Resolve a shield's Orchard output address and live-activity
/// classification from the optional third-party `recipient`.
///
/// `None` is the internal shield-to-self: the note goes to the
/// account's default address and the live entry is `Shield`/`In` with
/// no counterparty. `Some` pays a THIRD-PARTY address: `Sent`/`Out`
/// with the raw 43-byte address as counterparty — the exact
/// classification the scan deriver produces for an OVK-recovered send
/// to a non-own address, so a restore derives the same row.
///
/// A `Some` address the account's own IVK recognizes (default or any
/// diversified index — the same `diversifier_index` test the scan's
/// `is_own_orchard_recipient` uses) is rejected instead of classified:
/// its note WOULD be spendable here, so it is not a send, and
/// recording it live as `Sent`/`Out` while a restore scan-derives a
/// self-pay row would fork the two histories. Self-shields take the
/// `None` path.
fn resolve_shield_recipient(
    keys: &AccountViewingKeys,
    recipient: Option<&PaymentAddress>,
) -> Result<ShieldRecipient, PlatformWalletError> {
    match recipient {
        Some(payment_address) => {
            if keys
                .incoming_viewing_key
                .diversifier_index(payment_address)
                .is_some()
            {
                return Err(PlatformWalletError::ShieldedBuildError(
                    "recipient belongs to this shielded account; use the self-shield \
                     entry point (no recipient) instead"
                        .to_string(),
                ));
            }
            Ok(ShieldRecipient {
                address: payment_address_to_orchard(payment_address)?,
                counterparty: Some(payment_address.to_raw_address_bytes().to_vec()),
                kind: ShieldedActivityKind::Sent,
                direction: ShieldedDirection::Out,
            })
        }
        None => Ok(ShieldRecipient {
            address: default_orchard_address(keys)?,
            counterparty: None,
            kind: ShieldedActivityKind::Shield,
            direction: ShieldedDirection::In,
        }),
    }
}

/// Shield credits from transparent platform addresses into the
/// shielded pool, with the resulting note assigned to `account`'s
/// default Orchard payment address derived from `keys`.
///
/// Self-shield front for [`shield_to`], preserving the pre-recipient
/// signature for existing callers.
#[allow(clippy::too_many_arguments)]
pub async fn shield<S: ShieldedStore, Sig: Signer<PlatformAddress>, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &AccountViewingKeys,
    account: u32,
    inputs: BTreeMap<PlatformAddress, Credits>,
    amount: u64,
    signer: &Sig,
    prover: &P,
) -> Result<(), PlatformWalletError> {
    shield_to(
        sdk, store, persister, wallet_id, keys, account, None, inputs, amount,
        [0u8; 36], // empty memo
        signer, prover,
    )
    .await
}

/// Shield credits from transparent platform addresses into the
/// shielded pool. `recipient` selects the note's Orchard payment
/// address: `None` assigns it to `account`'s default address derived
/// from `keys` (the internal shield-to-self); `Some` pays a
/// third-party address — the note funds THAT wallet's pool and never
/// becomes spendable here (a `Some` address this account's own IVK
/// recognizes is rejected — see [`resolve_shield_recipient`]). Either
/// way the output is encrypted under our own OVK, so the scan recovers
/// the send from chain data and the live and scan-derived activity ids
/// line up.
#[allow(clippy::too_many_arguments)]
pub async fn shield_to<S: ShieldedStore, Sig: Signer<PlatformAddress>, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &AccountViewingKeys,
    account: u32,
    recipient: Option<&PaymentAddress>,
    inputs: BTreeMap<PlatformAddress, Credits>,
    amount: u64,
    memo: [u8; 36],
    signer: &Sig,
    prover: &P,
) -> Result<(), PlatformWalletError> {
    let ShieldRecipient {
        address: recipient_addr,
        counterparty: external_counterparty,
        kind,
        direction,
    } = resolve_shield_recipient(keys, recipient)?;
    let id = SubwalletId::new(wallet_id, account);

    // Reserve the flat shielded fee `F` on top of `amount` in the input
    // claims. Consensus `validate_structure` (rs-dpp) now rejects a Shield
    // unless `Σ inputs >= amount + F`, where
    // `F = compute_minimum_shielded_fee(num_actions)` and `num_actions` is
    // the serialized output-only bundle's action count. That bundle has a
    // single output and spends disabled, so Orchard pads it to exactly
    // SHIELD_NUM_ACTIONS == 2 actions (see the constant doc below). We
    // mirror `note_selection.rs`'s spend-side fee math.
    //
    // The fee is loaded onto the smallest-key input — the `DeductFromInput(0)`
    // fee-strategy payer (input 0 == BTreeMap-smallest address). The caller
    // (`shielded_shield_from_account`) reserves `shield_fee_reserve_credits`
    // (a small multiple of this same versioned fee) of unclaimed headroom on
    // input 0 specifically for this, so `F` always fits within the reserve.
    // Inflating the claim BEFORE the fetch lets the
    // single hard balance check below validate the fee-inclusive claim
    // against the on-chain balance in one shot — no second round-trip and
    // no claim that outruns its balance check.
    let fee = compute_minimum_shielded_fee(SHIELD_NUM_ACTIONS, sdk.version())
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
    let inputs = reserve_shield_fee_on_input_0(inputs, fee)?;

    // Reuse rs-sdk's canonical fetch + hard balance check rather than
    // re-implementing the fetch-and-validate dance. Unlike the old
    // warn-and-proceed path, `fetch_inputs_with_nonce` errors with
    // `AddressNotEnoughFundsError` when any input is short, so we fail
    // before paying the ~30 s Halo 2 proof for a transition drive-abci
    // would reject. It returns the *current* on-chain nonces; we apply
    // a checked increment (the canonical `nonce_inc` uses an unchecked
    // `+ 1`, which would wrap an address at u32::MAX into a replay
    // nonce — bail loudly here instead).
    use dash_sdk::platform::transition::fetch_inputs_with_nonce;

    let fetched = fetch_inputs_with_nonce(sdk, &inputs)
        .await
        .map_err(|error| map_shield_input_fetch_error(&error))?;

    let mut inputs_with_nonce: BTreeMap<PlatformAddress, (u32, Credits)> = BTreeMap::new();
    for (addr, (nonce, credits)) in fetched {
        let next_nonce = nonce.checked_add(1).ok_or_else(|| {
            PlatformWalletError::ShieldedBuildError(format!(
                "input address nonce exhausted on platform: {addr:?}"
            ))
        })?;
        inputs_with_nonce.insert(addr, (next_nonce, credits));
    }

    let fee_strategy: AddressFundsFeeStrategy =
        vec![AddressFundsFeeStrategyStep::DeductFromInput(0)];

    info!(
        account,
        credits = amount,
        external = external_counterparty.is_some(),
        "Shield: building proof"
    );

    let claimed_inputs = inputs_with_nonce.clone();

    let state_transition = build_shield_transition(
        &recipient_addr,
        amount,
        inputs_with_nonce,
        fee_strategy,
        signer,
        0, // user_fee_increase
        prover,
        memo,
        // Encrypt the output under the account's own OVK so the wallet's
        // shielded sync can recover this send (recipient, value, memo)
        // from chain data alone.
        Some(keys.outgoing_viewing_key.clone()),
        sdk.version(),
    )
    .await
    .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;

    trace!("Shield credits: state transition built, broadcasting...");
    let network = sdk.network;

    // Live activity. Kind / direction / counterparty were resolved
    // alongside the recipient above (see `resolve_shield_recipient` for
    // why the rows match what a restore's scan derives). Fee = the flat
    // shielded fee reserved above. The visible output cmx is the
    // recipient note (OVK-keyed either way), so the live and scan ids
    // line up.
    let pending_entry = record_pending_activity(
        store,
        persister,
        wallet_id,
        id,
        keys,
        LiveEntryParams {
            kind,
            direction,
            amount,
            fee: Some(fee),
            counterparty: external_counterparty,
            memo: non_zero_memo(&memo),
            actions: shielded_actions(&state_transition),
            spent_notes: &[],
        },
    )
    .await;

    // What an endpoint's answer says, for the reason an unconfirmed shield
    // reports. A funds shortfall names the claimed inputs beside what the
    // endpoint says Platform sees.
    let describe = |e: &dash_sdk::Error| -> String {
        let Some(rich) = addresses_not_enough_funds(e) else {
            return e.to_string();
        };
        let claimed = claimed_inputs
            .iter()
            .map(|(addr, (nonce, credits))| {
                format!(
                    "{}=(nonce {nonce}, {credits} credits)",
                    addr.to_bech32m_string(network)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "addresses not enough funds: required {} credits; \
             claimed inputs [{}]; platform sees [{}]",
            rich.required_balance(),
            claimed,
            format_addresses_with_info(rich.addresses_with_info(), network),
        )
    };

    // Broadcast keeping what every dispatch proved, then wait for the
    // verified snapshot of the input addresses (not just a relay ACK). A
    // shield's proof authenticates those addresses as of its block, not this
    // exact shield's execution, so the host sees success only once the
    // snapshot reads every input at or past the nonce this shield signed, and
    // the activity row stays pending until the scan observes the note.
    broadcast_shield(
        sdk,
        store,
        persister,
        wallet_id,
        id,
        &pending_entry,
        &state_transition,
        describe,
    )
    .await?;
    info!(
        account,
        credits = amount,
        "Shield admitted with its input addresses proved spent; the activity row awaits \
         on-chain confirmation by the shielded scan"
    );
    Ok(())
}

/// Broadcast the signed shield `state_transition`, keeping what every
/// dispatch proved ([`broadcast_tracking_delivery`]), wait for its result and
/// settle it ([`settle_shield`]).
#[allow(clippy::too_many_arguments)]
async fn broadcast_shield<S, D>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    state_transition: &StateTransition,
    describe: D,
) -> Result<(), PlatformWalletError>
where
    S: ShieldedStore,
    D: Fn(&dash_sdk::Error) -> String,
{
    let delivery = broadcast_tracking_delivery(sdk, state_transition).await;
    // A transition that names no input is no shield from addresses: its
    // result then shows nothing spent, and it settles as unconfirmed.
    let no_inputs = BTreeMap::new();
    settle_shield(
        store,
        persister,
        wallet_id,
        id,
        pending_entry,
        state_transition.inputs().unwrap_or(&no_inputs),
        delivery,
        describe,
        || state_transition.wait_for_affected_state::<StateTransitionProofResult>(sdk, None),
    )
    .await
}

/// What a signed shield's broadcast (`delivery`) and result wait (`wait`, run
/// only when a dispatch may have reached a node) came to, recording it on the
/// shield's activity row (`pending_entry`). `signed_inputs` are the shield's
/// input addresses with the nonces it signed, and `describe` renders an
/// endpoint's answer for the reason an unconfirmed shield reports.
///
/// Only a transition that never left the wallet
/// ([`BroadcastDelivery::NeverDelivered`]: refused before anything was sent)
/// is a failed shield
/// ([`PlatformWalletError::ShieldedBroadcastFailed`]): nothing holds its bytes
/// but the wallet, so it can never execute, its row reads failed and the host
/// may retry. Once a dispatch may have reached a node, every outcome short
/// of a verified result that reads its inputs spent
/// ([`shield_inputs_read_spent`]) is
/// [`PlatformWalletError::ShieldedSpendUnconfirmed`]: a wait that fails, a
/// result that does not show the inputs spent, an endpoint's refusal of the
/// broadcast and a consensus verdict on either call included. The endpoint
/// that sends a verdict is unauthenticated, no proof or quorum signature
/// backs it, and one that kept or forwarded the signed transition can answer
/// with any verdict, while a retry signs a second shield with the inputs'
/// next nonces, and a shield's note can be a recipient's ([`shield_to`]).
///
/// The row stays pending on every outcome but the failed one, a successful
/// one included, as a shield from an identity's does: the result proves the
/// input addresses as of its block, which does not bind this transition or
/// its note. The scan confirms the row when the note commitment appears on
/// chain.
///
/// The shield takes no note reservation and keeps no record of its signed
/// transition. What shows that an unconfirmed one can no longer execute is
/// its inputs' nonces (one of them used by another transition), which
/// nothing reads for it: until then the row stays pending.
#[allow(clippy::too_many_arguments)]
async fn settle_shield<S, D, W, Fut>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    signed_inputs: &BTreeMap<PlatformAddress, (AddressNonce, Credits)>,
    delivery: BroadcastDelivery,
    describe: D,
    wait: W,
) -> Result<(), PlatformWalletError>
where
    S: ShieldedStore,
    D: Fn(&dash_sdk::Error) -> String,
    W: FnOnce() -> Fut,
    Fut: Future<Output = Result<StateTransitionProofResult, dash_sdk::Error>>,
{
    let broadcast_answer = match delivery {
        BroadcastDelivery::Admitted => None,
        BroadcastDelivery::NeverDelivered(e) => {
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                pending_entry,
                ShieldedActivityStatus::Failed,
                None,
            )
            .await;
            return Err(definite_broadcast_failure(&e));
        }
        BroadcastDelivery::MaybeDelivered(e) => {
            warn!(
                account = id.account_index,
                error = %e,
                "Shield broadcast may have been delivered; falling through to the result wait"
            );
            Some(describe(&e))
        }
    };

    // Whatever the wait comes to, the activity row stays Pending: the scan's
    // confirmation pass flips it when the shielded note lands on-chain.
    let unproved = match wait().await {
        Ok(snapshot) => match shield_inputs_read_spent(signed_inputs, &snapshot) {
            Ok(()) => return Ok(()),
            Err(unproved) => {
                warn!(
                    account = id.account_index,
                    reason = %unproved,
                    "Shield result does not read its inputs spent; leaving the activity row pending"
                );
                unproved
            }
        },
        Err(wait_err) => {
            warn!(
                account = id.account_index,
                error = %wait_err,
                rejected = carries_consensus_rejection(&wait_err),
                "Shield broadcast but its result is unconfirmed; leaving the activity row pending"
            );
            describe(&wait_err)
        }
    };
    Err(PlatformWalletError::ShieldedSpendUnconfirmed {
        operation: "shield",
        reason: match broadcast_answer {
            Some(answer) => format!("{unproved}; its broadcast answered: {answer}"),
            None => unproved,
        },
    })
}

/// Whether `snapshot`, the verified result a shield's wait returned, reads
/// every input address of the shield at or past the nonce the shield signed
/// it with (`signed_inputs`), or why it does not.
///
/// The proof behind a shield's result authenticates its input addresses as
/// of the proof's block, not the shield's execution. An address's nonce
/// advances only when a transition signed with that nonce executes, so
/// inputs at or past their signed nonces show those nonces used. A result
/// that is no address snapshot shows nothing of the shield, and one that
/// leaves an input out, proves no state for it or reads it below its signed
/// nonce shows the shield had not executed at that block, whatever the
/// endpoint that returned it claims.
fn shield_inputs_read_spent(
    signed_inputs: &BTreeMap<PlatformAddress, (AddressNonce, Credits)>,
    snapshot: &StateTransitionProofResult,
) -> Result<(), String> {
    let StateTransitionProofResult::VerifiedAddressInfos(proved) = snapshot else {
        return Err(format!(
            "a proof of its input addresses was expected, received {snapshot}"
        ));
    };
    if signed_inputs.is_empty() {
        return Err("it names no input address".to_string());
    }
    for (address, (signed, _)) in signed_inputs {
        match proved.get(address) {
            Some(Some((proved, _))) if proved >= signed => {}
            Some(Some((proved, _))) => {
                return Err(format!(
                    "its result proves an input address at nonce {proved}, below the nonce \
                     {signed} it signed"
                ));
            }
            Some(None) | None => {
                return Err("its result proves no state for an input address".to_string());
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------
// ShieldFromIdentity: identity balance -> shielded pool (Type 21)
// -------------------------------------------------------------------------

/// Return whether this wallet already has an exact ShieldFromIdentity
/// transition whose outcome is unresolved. These records are persisted by
/// the file-backed store before broadcast, so a process restart cannot turn
/// an ambiguous debit into a fresh transition with the next identity nonce.
fn has_unresolved_identity_shield<S: ShieldedStore>(
    store: &S,
    wallet_id: WalletId,
    identity_id: [u8; 32],
) -> Result<bool, S::Error> {
    use dpp::serialization::PlatformDeserializableTrusted;
    use dpp::state_transition::shield_from_identity_transition::accessors::ShieldFromIdentityTransitionAccessorsV0;

    for (_, redrive) in store.pending_redrives_for_wallet(wallet_id)? {
        if redrive.identity_user_abandoned && redrive.nullifiers.is_empty() {
            continue;
        }
        match StateTransition::deserialize_from_bytes_trusted(&redrive.st_bytes) {
            Ok(StateTransition::ShieldFromIdentity(transition))
                if transition.identity_id().to_buffer() == identity_id =>
            {
                return Ok(true);
            }
            // An empty-nullifier redrive is reserved for identity-funded
            // shielding. If it is unreadable or decodes to another transition
            // kind, fail closed because its identity cannot be recovered safely.
            Err(_) if redrive.nullifiers.is_empty() => return Ok(true),
            Ok(StateTransition::ShieldFromIdentity(_)) => {}
            Ok(_) if redrive.nullifiers.is_empty() => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

/// Persist the exact signed identity debit before any network request. It is
/// safe to re-broadcast these bytes because their identity nonce is fixed; the
/// same record also blocks the public API from building a second debit while
/// the first outcome is unresolved.
async fn arm_identity_shield_redrive<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    state_transition: &StateTransition,
) -> Result<[u8; 32], PlatformWalletError> {
    use dpp::serialization::PlatformSerializable;
    use dpp::state_transition::shield_from_identity_transition::accessors::ShieldFromIdentityTransitionAccessorsV0;

    let entry = pending_entry.as_ref().ok_or_else(|| {
        PlatformWalletError::ShieldedBuildError(
            "ShieldFromIdentity produced no wallet-visible activity entry".to_string(),
        )
    })?;
    let StateTransition::ShieldFromIdentity(transition) = state_transition else {
        return Err(PlatformWalletError::ShieldedBuildError(
            "expected a ShieldFromIdentity transition".to_string(),
        ));
    };
    let st_bytes = state_transition.serialize_to_bytes().map_err(|e| {
        PlatformWalletError::ShieldedBuildError(format!(
            "serialize ShieldFromIdentity retry guard: {e}"
        ))
    })?;
    store
        .write()
        .await
        .arm_redrive(
            id,
            PendingRedrive {
                activity_id: entry.id,
                anchor: transition.anchor(),
                nullifiers: vec![],
                st_bytes,
                attempts: 0,
                identity_nonce_finalized: false,
                identity_user_abandoned: false,
                anchor_height: None,
                identity_index: None,
            },
        )
        .map_err(|e| PlatformWalletError::ShieldedStoreError(e.to_string()))?;
    Ok(entry.id)
}

/// Shield credits from a Platform identity's balance straight into the shielded
/// pool, with the resulting note assigned to `account`'s default Orchard address
/// (`recipient` `None`) or to a third-party Orchard address.
///
/// Unlike [`shield_to`] there are no transparent inputs, no fee strategy, and no
/// address witnesses: the identity's TRANSFER key signs the whole transition and
/// consensus debits `amount` plus the metered fee (note writes + identity writes)
/// plus the shielded compute fee from the identity balance. `nonce` is fetched
/// from Platform here, so the caller only supplies the identity.
///
/// The activity row records no fee: the exact fee is metered at execution and
/// never returned to the wallet, and `ShieldedActivityEntry::fee` is an exact
/// value (the FFI reports it as such), so the consensus floor must not stand in
/// for it.
///
/// Before broadcast, the bundle must produce a wallet-visible activity entry
/// and the exact signed transition must be persisted. Missing activity or a
/// failed durable guard write aborts the call without broadcasting. Once
/// submitted, remote failure verdicts never authorize building
/// a replacement: an unused nonce can still execute later, and a consumed nonce
/// may belong to this payment. The row stays pending until the scan observes its
/// outputs. Proven nonce finality stops rebroadcasts while retaining the guard.
/// Further calls for the same identity return
/// [`PlatformWalletError::ShieldedIdentityDebitPending`] before building a proof.
///
/// Returns the identity's proven post-debit balance, which requires the
/// identity's own `VerifiedPartialIdentity` proof carrying a balance (the SDK's
/// `ShieldFromIdentity` check); a proof for another identity, one without a
/// balance, or any other result reports `ShieldedSpendUnconfirmed`. That balance
/// proof is an affected-state snapshot: it binds neither this transition nor the
/// created note, so it does NOT confirm the activity row. The row stays `Pending`
/// until the shielded scan observes the note commitments on-chain (the
/// coordinator's confirmation pass), exactly like the ambiguous post-broadcast
/// paths. The caller owns the managed identity and persists the balance
/// (`PlatformWallet::shielded_shield_from_identity`).
#[allow(clippy::too_many_arguments)]
pub(in crate::wallet) async fn shield_from_identity_to<
    S: ShieldedStore,
    Sig: Signer<IdentityPublicKey>,
    P: OrchardProver,
>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &AccountViewingKeys,
    account: u32,
    recipient: Option<&PaymentAddress>,
    identity: &Identity,
    amount: u64,
    memo: [u8; 36],
    signer: &Sig,
    prover: &P,
) -> Result<(Credits, ResponseMetadata), PlatformWalletError> {
    let ShieldRecipient {
        address: recipient_addr,
        counterparty: external_counterparty,
        kind,
        direction,
    } = resolve_shield_recipient(keys, recipient)?;
    let id = SubwalletId::new(wallet_id, account);
    let identity_id = identity.id();

    let has_unresolved = {
        let store = store.read().await;
        has_unresolved_identity_shield(&*store, wallet_id, identity_id.to_buffer()).map_err(
            |e| {
                PlatformWalletError::ShieldedStoreError(format!(
                    "could not inspect unresolved identity debits: {e}"
                ))
            },
        )?
    };
    if has_unresolved {
        return Err(PlatformWalletError::ShieldedIdentityDebitPending {
            identity_id: identity_id.to_buffer(),
        });
    }

    // A self-shield funded by an identity is its own activity kind so the
    // history shows which identity paid; a third-party recipient stays `Sent`.
    let kind = match kind {
        ShieldedActivityKind::Shield => ShieldedActivityKind::ShieldFromIdentity {
            identity_id: identity_id.to_buffer(),
        },
        other => other,
    };

    let nonce = sdk
        .get_identity_nonce(identity_id, true, None)
        .await
        .map_err(|e| {
            PlatformWalletError::ShieldedBuildError(format!("fetch identity nonce: {e}"))
        })?;

    info!(
        account,
        credits = amount,
        identity = %identity_id,
        external = external_counterparty.is_some(),
        "ShieldFromIdentity: building proof"
    );

    let state_transition = build_shield_from_identity_transition(
        identity,
        &recipient_addr,
        amount,
        nonce,
        signer,
        None,
        0, // user_fee_increase
        prover,
        memo,
        Some(keys.outgoing_viewing_key.clone()),
        sdk.version(),
    )
    .await
    .map_err(map_shield_from_identity_build_error)?;

    trace!("ShieldFromIdentity: state transition built, broadcasting...");

    let pending_entry = record_pending_activity(
        store,
        persister,
        wallet_id,
        id,
        keys,
        LiveEntryParams {
            kind,
            direction,
            amount,
            // Exact fee unknown (metered at execution); see the function docs.
            fee: None,
            counterparty: external_counterparty,
            memo: non_zero_memo(&memo),
            actions: shielded_actions(&state_transition),
            spent_notes: &[],
        },
    )
    .await;

    let activity_id =
        arm_identity_shield_redrive(store, id, &pending_entry, &state_transition).await?;

    match state_transition.broadcast(sdk, None).await {
        Ok(()) => {}
        Err(e) if broadcast_was_refused(&e) => {
            // Unauthenticated verdict: settle it against the proven nonce.
            return Err(settle_identity_shield_failure(
                sdk,
                store,
                id,
                activity_id,
                identity_id,
                nonce,
                e,
            )
            .await);
        }
        Err(e) => {
            warn!(
                account,
                error = %e,
                "ShieldFromIdentity broadcast returned no verdict; the transition may have \
                 been admitted; falling through to the result wait"
            );
        }
    }

    // The proof authenticates the identity's post-debit balance (an affected-state
    // snapshot, not execution evidence). Remote rejection errors are not
    // authenticated and therefore leave the debit unresolved.
    // `wait_for_affected_state` only converts the proof generically, so the variant
    // and the identity are enforced here: only this identity's balance proof is
    // accepted as the post-debit balance.
    let proof_outcome: Result<(Credits, ResponseMetadata), String> = match state_transition
        .wait_for_affected_state_with_metadata::<StateTransitionProofResult>(sdk, None)
        .await
    {
        Ok((StateTransitionProofResult::VerifiedPartialIdentity(partial), metadata))
            if partial.id == identity_id =>
        {
            partial
                .balance
                .ok_or_else(|| "the identity proof did not include the updated balance".to_string())
                .map(|balance| (balance, metadata))
        }
        Ok((StateTransitionProofResult::VerifiedPartialIdentity(partial), _)) => Err(format!(
            "the proof returned identity {} but {} initiated the shield",
            partial.id, identity_id
        )),
        Ok(other) => Err(format!(
            "an identity balance proof was expected, received {other:?}"
        )),
        Err(wait_err) => {
            if carries_consensus_rejection(&wait_err) {
                // The wait-side error envelope is unauthenticated too: settle it
                // against the proven nonce before recording a failure.
                return Err(settle_identity_shield_failure(
                    sdk,
                    store,
                    id,
                    activity_id,
                    identity_id,
                    nonce,
                    wait_err,
                )
                .await);
            }
            Err(wait_err.to_string())
        }
    };
    let new_balance = match proof_outcome {
        Ok(balance) => balance,
        Err(reason) => {
            warn!(
                account,
                %reason,
                "ShieldFromIdentity broadcast accepted but result confirmation failed; \
                 leaving the activity row pending"
            );
            return Err(PlatformWalletError::ShieldedSpendUnconfirmed {
                operation: "shield from identity",
                reason,
            });
        }
    };

    // Not confirmed here: the balance proof does not bind this transition or its
    // note, so the row stays `Pending` until the scan's confirmation pass observes
    // the note commitments on-chain (see the function docs).
    info!(
        account,
        credits = amount,
        identity = %identity_id,
        "ShieldFromIdentity admitted with a proven post-debit balance; the activity row \
         awaits on-chain confirmation by the shielded scan"
    );
    Ok(new_balance)
}

/// Whether a proven identity nonce word still leaves `used` available to execute.
/// This is a snapshot, not a promise of non-execution: a relayed transaction can
/// consume an unused nonce after the proof was fetched.
///
/// Platform stores the identity nonce as the highest merged value (the low
/// `IDENTITY_NONCE_VALUE_FILTER` bits) plus a bitmask of the up to
/// `MAX_MISSING_IDENTITY_REVISIONS` values below it that were skipped (bit
/// `position - 1 + IDENTITY_NONCE_VALUE_FILTER_MAX_BYTES` set means `tip - position`
/// is still missing). `used` is proven unmerged when it lies above the tip, or when it
/// lies within the window below the tip and its missing bit is set. A value older than
/// the window can no longer be told apart from a merged one, so it is treated as
/// possibly used.
pub(crate) fn identity_nonce_proves_unused(stored: IdentityNonce, used: IdentityNonce) -> bool {
    let tip = stored & IDENTITY_NONCE_VALUE_FILTER;
    if used > tip {
        return true;
    }
    if used == tip {
        return false;
    }
    let position = tip - used;
    if position > MAX_MISSING_IDENTITY_REVISIONS {
        return false;
    }
    let missing = stored & MISSING_IDENTITY_REVISIONS_FILTER;
    missing & (1u64 << (position - 1 + IDENTITY_NONCE_VALUE_FILTER_MAX_BYTES)) != 0
}

/// Park exact-byte retries once a proof makes the nonce unavailable. Both
/// outcomes retain the payment guard: unused may execute later, while finalized
/// may already have executed. Never resurrect a guard the scan already cleared.
fn reconcile_identity_shield_nonce<S: ShieldedStore>(
    store: &mut S,
    id: SubwalletId,
    activity_id: [u8; 32],
    stored_nonce: Option<IdentityNonce>,
    nonce: IdentityNonce,
) -> Result<bool, S::Error> {
    if stored_nonce
        .map(|stored| identity_nonce_proves_unused(stored, nonce))
        .unwrap_or(true)
    {
        return Ok(false);
    }
    if let Some(mut redrive) = store
        .pending_redrives(id)?
        .into_iter()
        .find(|redrive| redrive.activity_id == activity_id)
    {
        if !redrive.identity_nonce_finalized && !redrive.identity_user_abandoned {
            redrive.identity_nonce_finalized = true;
            store.arm_redrive(id, redrive)?;
        }
    }
    Ok(true)
}

/// A relay verdict, including an apparent consensus rejection, cannot prove
/// non-execution. A valid nonce proof may stop futile rebroadcasting but can
/// never turn that verdict into permission for a fresh-nonce replacement.
async fn settle_identity_shield_failure<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    activity_id: [u8; 32],
    identity_id: Identifier,
    nonce: IdentityNonce,
    verdict: dash_sdk::Error,
) -> PlatformWalletError {
    let reason = match IdentityNonceFetcher::fetch(sdk, identity_id).await {
        Ok(fetched) => {
            let mut store = store.write().await;
            match reconcile_identity_shield_nonce(
                &mut *store,
                id,
                activity_id,
                fetched.map(|fetcher| fetcher.0),
                nonce,
            ) {
                Ok(true) => {
                    "the nonce can no longer execute; awaiting output confirmation".to_string()
                }
                Ok(false) => {
                    "the nonce is unused now, but the signed debit may still execute later"
                        .to_string()
                }
                Err(e) => format!("could not persist nonce reconciliation: {e}"),
            }
        }
        Err(e) => format!("the identity nonce could not be verified: {e}"),
    };
    PlatformWalletError::ShieldedSpendUnconfirmed {
        operation: "shield from identity",
        reason: format!("{verdict}; {reason}; the original payment remains pending"),
    }
}

#[cfg(test)]
mod identity_nonce_proves_unused_tests {
    use super::*;

    fn missing_bit(position: u64) -> u64 {
        1u64 << (position - 1 + IDENTITY_NONCE_VALUE_FILTER_MAX_BYTES)
    }

    /// Above the tip: never merged. At the tip: merged.
    #[test]
    fn tip_comparisons() {
        assert!(
            identity_nonce_proves_unused(0, 1),
            "fresh identity, nonce 1 unused"
        );
        assert!(identity_nonce_proves_unused(5, 6));
        assert!(!identity_nonce_proves_unused(5, 5));
    }

    /// Below the tip within the window: unused only when its missing bit is set.
    #[test]
    fn window_below_tip_reads_the_missing_mask() {
        let tip = 10u64;
        // nonce 8 is two below the tip: position 2.
        assert!(!identity_nonce_proves_unused(tip, 8), "no mask: merged");
        assert!(identity_nonce_proves_unused(tip | missing_bit(2), 8));
        assert!(
            !identity_nonce_proves_unused(tip | missing_bit(3), 8),
            "a different position's bit must not count"
        );
    }

    /// Older than the window: cannot be told apart from merged, so possibly used.
    #[test]
    fn older_than_window_is_possibly_used() {
        let tip = 100u64;
        assert!(!identity_nonce_proves_unused(
            tip,
            tip - MAX_MISSING_IDENTITY_REVISIONS - 1
        ));
        assert!(identity_nonce_proves_unused(
            tip | missing_bit(MAX_MISSING_IDENTITY_REVISIONS),
            tip - MAX_MISSING_IDENTITY_REVISIONS
        ));
    }
}

/// Map a `ShieldFromIdentity` builder failure. The builder signs with the
/// identity signer, so a structured key-unavailable completion (the reserved
/// marker at position 0 of a `ProtocolError::Generic` payload) can surface
/// here; it is preserved verbatim under [`PlatformWalletError::Sdk`] so the
/// FFI boundary restores code 31 (`ErrorSigningKeyUnavailable`) instead of
/// flattening it into the generic build error. Every other failure keeps the
/// protocol error's own rendering as a `ShieldedBuildError`.
fn map_shield_from_identity_build_error(error: ProtocolError) -> PlatformWalletError {
    preserve_signer_key_unavailable_or(dash_sdk::Error::Protocol(error), |e| match e {
        dash_sdk::Error::Protocol(protocol_error) => {
            PlatformWalletError::ShieldedBuildError(protocol_error.to_string())
        }
        other => PlatformWalletError::ShieldedBuildError(other.to_string()),
    })
}

#[cfg(test)]
mod shield_from_identity_build_error_tests {
    use super::*;
    use crate::error::SIGNER_KEY_UNAVAILABLE_PREFIX;

    /// A key-unavailable signer completion keeps its structured shape so the
    /// FFI can restore code 31; the marker must sit at position 0.
    #[test]
    fn preserves_signer_key_unavailable_completion() {
        let error = ProtocolError::Generic(format!(
            "{SIGNER_KEY_UNAVAILABLE_PREFIX}transfer key not in keychain"
        ));
        let mapped = map_shield_from_identity_build_error(error);
        assert!(
            matches!(
                &mapped,
                PlatformWalletError::Sdk(dash_sdk::Error::Protocol(ProtocolError::Generic(s)))
                    if s.starts_with(SIGNER_KEY_UNAVAILABLE_PREFIX)
            ),
            "expected the signer completion preserved under Sdk, got {mapped:?}"
        );
    }

    /// Any other builder failure is a build error carrying the protocol
    /// error's own rendering (the SDK wrapper's "Protocol error:" prefix is
    /// not added), including a generic error that merely mentions the marker
    /// mid-string.
    #[test]
    fn stringifies_other_builder_failures() {
        let mapped = map_shield_from_identity_build_error(ProtocolError::Generic(
            "amount must be > 0".to_string(),
        ));
        assert!(
            matches!(
                &mapped,
                PlatformWalletError::ShieldedBuildError(s) if s == "Generic Error: amount must be > 0"
            ),
            "got {mapped:?}"
        );

        let mid_string = map_shield_from_identity_build_error(ProtocolError::Generic(format!(
            "signer failed: {SIGNER_KEY_UNAVAILABLE_PREFIX}not at position 0"
        )));
        assert!(
            matches!(mid_string, PlatformWalletError::ShieldedBuildError(_)),
            "a mid-string marker must not be promoted, got {mid_string:?}"
        );
    }
}

// -------------------------------------------------------------------------
// IdentityTopUpFromShieldedPool: shielded pool -> existing identity (Type 22)
// -------------------------------------------------------------------------

/// Top up an existing Platform identity's balance from `account`'s shielded
/// notes. Mirrors [`unshield`]: notes are reserved for `amount + fee` under
/// [`ShieldedFeeKind::IdentityTopUp`], the spend bundle binds the identity and
/// gross amount into its sighash, and the broadcast is redrive-safe. The
/// identity receives `amount` (the fee is carved from the value balance).
/// Waits for proven execution before marking notes spent.
///
/// Returns the identity's proof-attested post-top-up balance when the result
/// proof carried this identity (`None` only if the proven result had an
/// unexpected shape, which is logged). The caller persists it for a managed
/// identity (`PlatformWallet::shielded_identity_top_up_from_pool`).
#[allow(clippy::too_many_arguments)]
pub async fn identity_top_up_from_pool<S: ShieldedStore, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    identity_id: Identifier,
    amount: u64,
    prover: &P,
) -> Result<Option<Credits>, PlatformWalletError> {
    identity_top_up_from_pool_with_metadata(
        sdk,
        store,
        persister,
        wallet_id,
        keys,
        account,
        identity_id,
        amount,
        prover,
    )
    .await
    .map(|(result, _)| result)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::wallet) async fn identity_top_up_from_pool_with_metadata<
    S: ShieldedStore,
    P: OrchardProver,
>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    identity_id: Identifier,
    amount: u64,
    prover: &P,
) -> Result<(Option<Credits>, ResponseMetadata), PlatformWalletError> {
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);

    let (selected_notes, total_input, exact_fee, mut reservation) = reserve_unspent_notes(
        sdk,
        store,
        id,
        amount,
        2,
        ShieldedFeeKind::IdentityTopUp,
        // The top-up builder sends any remainder as change, so exact coverage is fundable.
        ChangeRequirement::Optional,
    )
    .await?;

    info!(
        account,
        credits = amount,
        fee = exact_fee,
        inputs = selected_notes.len(),
        total_input,
        identity = %identity_id,
        "IdentityTopUpFromShieldedPool"
    );

    let mut pending_entry = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        let anchor_bytes = anchor.to_bytes();

        let (state_transition, fee_used) = build_identity_top_up_from_shielded_pool_transition(
            spends,
            identity_id,
            amount,
            &change_addr,
            &keys.full_viewing_key,
            &keys.spend_auth_key,
            anchor,
            prover,
            [0u8; 36],
            sdk.version(),
        )
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved identity top up fee"
        );

        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind: ShieldedActivityKind::IdentityTopUp {
                    identity_id: identity_id.to_buffer(),
                },
                direction: ShieldedDirection::Out,
                amount,
                fee: Some(fee_used),
                counterparty: Some(identity_id.to_vec()),
                memo: None,
                actions: shielded_actions(&state_transition),
                spent_notes: &selected_notes,
            },
        )
        .await;

        trace!("IdentityTopUpFromShieldedPool: state transition built, saving and broadcasting...");
        // Type 22 is classified affected state: its nullifiers are shared with any
        // competing spend of the same notes and the identity's balance is a
        // snapshot, so the strict wait would reject every success. The snapshot
        // still proves the reserved notes are consumed and authenticates the
        // credited identity's balance; the shield, shield-from-identity and
        // identity-create paths accept the same class of outcome.
        broadcast_shielded_spend_with_redrive_with_metadata(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &state_transition,
            "identity top up from shielded pool",
            SpendResultWait::AffectedState,
        )
        .await
    }
    .await;

    match result {
        Ok((proof, metadata)) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "IdentityTopUpFromShieldedPool broadcast succeeded but local spent-state \
                     update failed; will heal on next sync"
                );
            }
            info!(account, credits = amount, identity = %identity_id, "IdentityTopUpFromShieldedPool broadcast succeeded");
            // The affected-state wait returns the spent nullifiers plus the
            // credited identity as a snapshot at the proof's block. The notes are
            // spent either way; the identity's proven balance is the balance at
            // that block, never derived from the requested amount.
            //
            // The row is confirmed here on purpose. The snapshot cannot tell this
            // top-up apart from a competing spend of the same notes, but only this
            // wallet's spending key can author one and the notes were reserved
            // locally, so spent nullifiers are treated as this transition's
            // execution, the same policy the transfer, unshield and withdrawal
            // paths apply. Until the protocol stores a transition-bound receipt
            // (a follow-up that would make every nullifier-spend family
            // execution-proving), a second device on the same seed can show a
            // confirmed row for a top-up its sibling displaced; immediate
            // confirmation was chosen over a pending row for that edge case.
            let proven_balance = match proof {
                StateTransitionProofResult::VerifiedIdentityWithShieldedNullifiers(proven, _)
                    if proven.id() == identity_id =>
                {
                    Some(proven.balance())
                }
                other => {
                    warn!(
                        account,
                        identity = %identity_id,
                        result = ?other,
                        "IdentityTopUpFromShieldedPool proof did not carry the credited \
                         identity; managed balance left for the next identity refresh"
                    );
                    None
                }
            };
            Ok((proven_balance, metadata))
        }
        Err(failure) => Err(settle_failed_spend(
            store,
            persister,
            wallet_id,
            id,
            &pending_entry,
            reservation,
            "identity top up from shielded pool",
            failure,
        )
        .await),
    }
}

// -------------------------------------------------------------------------
// PrivateDocumentCreateFromShieldedPool: shielded pool -> entries of a
// private document type's store (Type 29)
// -------------------------------------------------------------------------

/// Append `entries` to the store of the private document type
/// `document_type_name` of `data_contract_id` (`PrivateDocumentCreateFromShieldedPool`,
/// type 29), paying the fee from `account`'s notes, the change going back to
/// the account. Nothing but the fee leaves the pool. Every entry must take the
/// type's `entrySize`, as `platform_encryption::seal_private_document_entry`
/// builds them.
///
/// Waits for the spent nullifiers to be proven before marking the notes
/// spent, and returns the metadata of that proof. `payment_started` is
/// marked right before the transition is broadcast.
#[allow(clippy::too_many_arguments)]
pub(in crate::wallet) async fn private_document_create_from_pool_with_metadata<
    S: ShieldedStore,
    P: OrchardProver,
>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    data_contract_id: Identifier,
    document_type_name: &str,
    entries: Vec<BinaryData>,
    private_contact: Option<PrivateContactPayment>,
    payment_started: &PaymentStarted,
    prover: &P,
) -> Result<ResponseMetadata, PlatformWalletError> {
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);
    let fee_kind = ShieldedFeeKind::PrivateDocumentCreate {
        entry_count: entries.len(),
        entry_size: entries.first().map_or(0, |entry| entry.len() as u32),
    };

    let (selected_notes, total_input, exact_fee, reservation) =
        reserve_notes_covering(sdk, store, id, 0, 2, fee_kind, ChangeRequirement::Optional).await?;

    info!(
        account,
        fee = exact_fee,
        entries = entries.len(),
        inputs = selected_notes.len(),
        total_input,
        data_contract = %data_contract_id,
        document_type = document_type_name,
        "PrivateDocumentCreateFromShieldedPool"
    );

    // The journal records of a private contact request payment, keyed by
    // these commitments, are discharged here only when the transition never
    // left the wallet.
    let journaled = private_contact.map(|_| {
        entries
            .iter()
            .filter_map(|entry| {
                entry
                    .as_slice()
                    .get(..32)
                    .and_then(|commitment| <[u8; 32]>::try_from(commitment).ok())
            })
            .collect::<Vec<[u8; 32]>>()
    });
    let built = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        let (state_transition, fee_used) =
            build_private_document_create_from_shielded_pool_transition(
                spends,
                data_contract_id,
                document_type_name.to_string(),
                entries,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                sdk.version(),
            )
            .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved private document create fee"
        );
        Ok::<_, PlatformWalletError>((state_transition, fee_used, anchor.to_bytes(), anchor_height))
    }
    .await;
    let (state_transition, fee_used, anchor_bytes, anchor_height) = match built {
        Ok(built) => built,
        Err(e) => {
            // Nothing was saved or broadcast.
            reservation.cancel().await;
            return Err(e);
        }
    };
    // The spend's only outputs are change, so it records as an own spend
    // with its fee and no counterparty.
    let pending_entry = record_pending_activity(
        store,
        persister,
        wallet_id,
        id,
        &views,
        LiveEntryParams {
            kind: ShieldedActivityKind::ShieldedSpend,
            direction: ShieldedDirection::Out,
            amount: 0,
            fee: Some(fee_used),
            counterparty: None,
            memo: None,
            actions: shielded_actions(&state_transition),
            spent_notes: &selected_notes,
        },
    )
    .await;
    // Type 29 proves its spent nullifiers as a snapshot, which a competing
    // spend of the same notes shares, as type 22's does: the affected-state
    // wait accepts it (see `identity_top_up_from_pool_with_metadata`).
    submit_private_document_payment(
        PrivateDocumentSubmission {
            store,
            persister,
            wallet_id,
            id,
            pending_entry,
            selected_notes: &selected_notes,
            reservation,
            state_transition,
            anchor: anchor_bytes,
            anchor_height,
            private_contact,
            journaled: journaled.as_deref(),
            payment_started,
        },
        |state_transition| async move {
            broadcast_shielded_spend_classified(
                sdk,
                &state_transition,
                "private document create from shielded pool",
                SpendResultWait::AffectedState,
            )
            .await
        },
    )
    .await
}

/// A built private document create, and everything submitting it takes
/// ([`submit_private_document_payment`]).
struct PrivateDocumentSubmission<'a, S: ShieldedStore> {
    store: &'a Arc<RwLock<S>>,
    persister: Option<&'a WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    /// The spend's activity entry, recorded pending; it keys its records.
    pending_entry: Option<super::activity::ShieldedActivityEntry>,
    selected_notes: &'a [ShieldedNote],
    reservation: NoteReservation<S>,
    state_transition: StateTransition,
    anchor: [u8; 32],
    /// The proved height of the recorded-anchor set `anchor` was chosen from.
    anchor_height: u64,
    private_contact: Option<PrivateContactPayment>,
    journaled: Option<&'a [[u8; 32]]>,
    payment_started: &'a PaymentStarted,
}

/// Record, save and broadcast a built private document create, then settle
/// its notes and saved record ([`settle_private_document_payment`]).
/// `broadcast` sends the transition and waits for its result: the production
/// flow passes the classified broadcast, a test intercepts it.
///
/// Saved before the broadcast, not after an unconfirmed wait: a process that
/// exits once the node admitted the transition, before the wait returns,
/// still leaves the record a restart restores the request's reservation from,
/// and the redrive resolves. The payment is marked started once the record is
/// saved, with nothing awaited in between, and the notes are handed over only
/// then: a send dropped while the save waits for the store has saved and
/// broadcast nothing, and its guard releases the notes. A save that fails
/// stops the payment before the broadcast.
async fn submit_private_document_payment<S, B, Fut>(
    submission: PrivateDocumentSubmission<'_, S>,
    broadcast: B,
) -> Result<ResponseMetadata, PlatformWalletError>
where
    S: ShieldedStore,
    B: FnOnce(StateTransition) -> Fut,
    Fut: Future<Output = Result<(StateTransitionProofResult, ResponseMetadata), SpendFailure>>,
{
    let PrivateDocumentSubmission {
        store,
        persister,
        wallet_id,
        id,
        pending_entry,
        selected_notes,
        mut reservation,
        state_transition,
        anchor,
        anchor_height,
        private_contact,
        journaled,
        payment_started,
    } = submission;
    // The wallet holds the pair before the payment is journaled, beyond its
    // sender identity, until the journal records are discharged.
    let mut pair_hold = JournaledPairHold::note(persister, private_contact, journaled);
    let result = async {
        let anchored = arm_pending_release(
            store,
            id,
            anchor,
            anchor_height,
            &pending_entry,
            selected_notes,
        )
        .await?;
        arm_redrive_before_broadcast(
            store,
            id,
            pending_entry.as_ref().map(|entry| entry.id),
            anchor,
            selected_notes.iter().map(|note| note.nullifier).collect(),
            &state_transition,
            "private document create from shielded pool",
            private_contact,
            Some(payment_started),
            anchor_height,
        )
        .await?;
        pair_hold.saved();
        if anchored {
            // Each reservation carries its anchor: a pruned anchor releases it.
            reservation.hand_over();
        }
        trace!("PrivateDocumentCreateFromShieldedPool: state transition saved, broadcasting...");
        broadcast(state_transition).await
    }
    .await;

    drop(pair_hold);
    settle_private_document_payment(
        store,
        persister,
        wallet_id,
        id,
        &pending_entry,
        reservation,
        selected_notes,
        journaled,
        payment_started,
        result.map(|(_proof, metadata)| metadata),
    )
    .await
}

/// The wallet's hold on a private contact request payment's pair, noted
/// before the payment's journal records are saved
/// ([`WalletPersister::note_journaled_private_payments`]), so the pairs held
/// cover every record the journal keeps. Dropped before [`Self::saved`], the
/// records were not saved and nothing was paid: the hold is forgotten.
struct JournaledPairHold<'a> {
    persister: Option<&'a WalletPersister>,
    commitments: &'a [[u8; 32]],
    saved: bool,
}

impl<'a> JournaledPairHold<'a> {
    /// Hold `payment`'s pair for `commitments`, the entries it appends, on
    /// `persister`. Holds nothing for a payment that is not for a private
    /// contact request.
    fn note(
        persister: Option<&'a WalletPersister>,
        payment: Option<PrivateContactPayment>,
        commitments: Option<&'a [[u8; 32]]>,
    ) -> Self {
        let (Some(persister), Some(payment), Some(commitments)) = (persister, payment, commitments)
        else {
            return Self {
                persister: None,
                commitments: &[],
                saved: true,
            };
        };
        let (sender, recipient) = (
            Identifier::from(payment.sender),
            Identifier::from(payment.recipient),
        );
        persister.note_journaled_private_payments(
            commitments
                .iter()
                .map(|commitment| (*commitment, sender, recipient)),
        );
        Self {
            persister: Some(persister),
            commitments,
            saved: false,
        }
    }

    /// The journal records were saved: the hold stays until they are
    /// discharged.
    fn saved(&mut self) {
        self.saved = true;
    }
}

impl Drop for JournaledPairHold<'_> {
    fn drop(&mut self) {
        if self.saved {
            return;
        }
        if let Some(persister) = self.persister {
            persister.forget_journaled_private_payments(self.commitments);
        }
    }
}

/// Settle a private document payment's notes and saved record once its
/// broadcast and result wait returned `result`, as
/// [`private_document_create_from_pool_with_metadata`] does.
///
/// A payment that executed marks its notes spent; one that may have executed
/// (unconfirmed) keeps them reserved and its saved record, which the redrive
/// pass resolves; one that did not execute releases them, and its record with
/// them. The private payment journal's records of a private contact request
/// payment (`journaled`, its entries' commitments) outlive all of that: they
/// are discharged here only when the transition never left the wallet
/// ([`SpendFailure::never_delivered`]), as nothing else can then execute it,
/// and `payment_started` tells the caller so
/// ([`PaymentStarted::note_never_delivered`]). Otherwise they stay until the
/// entry is read back.
///
/// A transition that never left the wallet has its saved record, keyed by
/// its activity entry's id (`pending_entry`), retired with the journal
/// records in one durable write, under the store lock the redrive pass claims
/// each re-broadcast under ([`NoteReservation::retire_undelivered`]). One
/// whose record a redrive pass claimed a re-broadcast of meanwhile, or that
/// cannot be retired, may leave the wallet after all: it is settled as
/// unconfirmed, keeping its notes, record and journal records.
///
/// A retirement is published at once, with nothing awaited in between: the
/// discharged commitments on `persister` and the never-delivered notice on
/// `payment_started`. The activity row is updated after, so a settlement
/// cancelled while it waits for the store still leaves the pair free.
#[allow(clippy::too_many_arguments)]
async fn settle_private_document_payment<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    mut reservation: NoteReservation<S>,
    selected_notes: &[ShieldedNote],
    journaled: Option<&[[u8; 32]]>,
    payment_started: &PaymentStarted,
    result: Result<ResponseMetadata, SpendFailure>,
) -> Result<ResponseMetadata, PlatformWalletError> {
    match result {
        Ok(metadata) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, selected_notes).await
            {
                warn!(
                    account = id.account_index,
                    error = %e,
                    "PrivateDocumentCreateFromShieldedPool broadcast succeeded but local \
                     spent-state update failed; will heal on next sync"
                );
            }
            info!(
                account = id.account_index,
                "PrivateDocumentCreateFromShieldedPool broadcast succeeded"
            );
            Ok(metadata)
        }
        Err(SpendFailure {
            error: e @ PlatformWalletError::ShieldedSpendUnconfirmed { .. },
            ..
        }) => {
            // It may have executed: the reservation stays, released by sync.
            reservation.hand_over();
            Err(e)
        }
        Err(SpendFailure {
            error,
            never_delivered: true,
        }) => {
            let commitments = journaled.unwrap_or_default();
            let retired = match pending_entry.as_ref().map(|entry| entry.id) {
                Some(activity_id) => {
                    reservation
                        .retire_undelivered(&activity_id, commitments)
                        .await
                }
                None => {
                    reservation.hand_over();
                    false
                }
            };
            if !retired {
                // It may leave the wallet after all: everything stays, as for
                // an unconfirmed payment.
                return Err(PlatformWalletError::ShieldedSpendUnconfirmed {
                    operation: "private document create from shielded pool",
                    reason: format!(
                        "its broadcast was refused before delivery ({error}), but its saved \
                         transition was claimed for a re-broadcast or could not be retired"
                    ),
                });
            }
            // Published before anything is awaited: retired, nothing can
            // execute it, and a settlement dropped from here on must still
            // leave its pair free.
            if journaled.is_some() {
                if let Some(persister) = persister {
                    persister.note_discharged_private_entries(commitments.iter().copied());
                }
                payment_started.note_never_delivered();
            }
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                pending_entry,
                ShieldedActivityStatus::Failed,
                None,
            )
            .await;
            Err(error)
        }
        Err(SpendFailure { error, .. }) => {
            // It did not execute: the guard owns the notes until they are
            // released, so a drop on the way still releases them.
            reservation.reclaim();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                pending_entry,
                ShieldedActivityStatus::Failed,
                None,
            )
            .await;
            reservation.cancel().await;
            Err(error)
        }
    }
}

/// How the broadcast of [`pay_private_document_with_outcome`]'s payment ends.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum TestBroadcastOutcome {
    /// Refused before anything was delivered.
    RefusedBeforeDelivery,
    /// Delivered, its result wait timed out.
    Unconfirmed,
}

/// A private document payment from `notes` whose broadcast ends as `outcome`,
/// saved and settled as the private flow does
/// ([`submit_private_document_payment`]): for tests outside this module that
/// drive a settlement through a contact request's pair.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn pay_private_document_with_outcome<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    notes: &[ShieldedNote],
    pending_entry: super::activity::ShieldedActivityEntry,
    state_transition: StateTransition,
    private_contact: PrivateContactPayment,
    payment_started: &PaymentStarted,
    outcome: TestBroadcastOutcome,
) -> Result<ResponseMetadata, PlatformWalletError> {
    // Reserved and recorded pending, as the private flow does before it
    // submits.
    let reservation = {
        let mut locked = store.write().await;
        locked
            .save_activity(id, &pending_entry)
            .map_err(|e| PlatformWalletError::ShieldedStoreError(e.to_string()))?;
        mark_notes_pending(&mut *locked, store, id, notes)?
    };
    let journaled = private_document_commitments(&state_transition).ok_or_else(|| {
        PlatformWalletError::ShieldedBuildError("the payment's entries do not read".to_string())
    })?;
    submit_private_document_payment(
        PrivateDocumentSubmission {
            store,
            persister,
            wallet_id,
            id,
            pending_entry: Some(pending_entry),
            selected_notes: notes,
            reservation,
            state_transition,
            anchor: [7; 32],
            anchor_height: 0,
            private_contact: Some(private_contact),
            journaled: Some(&journaled),
            payment_started,
        },
        move |_| {
            std::future::ready(Err(match outcome {
                TestBroadcastOutcome::RefusedBeforeDelivery => SpendFailure {
                    error: PlatformWalletError::ShieldedBroadcastFailed(
                        "refused before delivery".to_string(),
                    ),
                    never_delivered: true,
                },
                TestBroadcastOutcome::Unconfirmed => SpendFailure {
                    error: PlatformWalletError::ShieldedSpendUnconfirmed {
                        operation: "private document create from shielded pool",
                        reason: "the result wait timed out".to_string(),
                    },
                    never_delivered: false,
                },
            }))
        },
    )
    .await
}

// -------------------------------------------------------------------------
// ShieldFromAssetLock: Core L1 asset lock -> shielded pool (Type 18)
// (orchestrated entry point lives in `wallet/shielded/fund_from_asset_lock.rs`)
// -------------------------------------------------------------------------

// -------------------------------------------------------------------------
// Unshield: shielded pool -> platform address (Type 17)
// -------------------------------------------------------------------------

/// Unshield funds from `account`'s shielded notes to a
/// transparent platform address.
#[allow(clippy::too_many_arguments)]
pub async fn unshield<S: ShieldedStore, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    to_address: &PlatformAddress,
    amount: u64,
    prover: &P,
) -> Result<(), PlatformWalletError> {
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);

    // Reserve against the 2-action floor: Orchard's BundleType::DEFAULT pads single-spend
    // bundles to 2 actions, and the builder prices the fee at spends.len().max(2). Reserving
    // for 1 would under-fee a single-note transition and the builder would reject it locally.
    // Unshield is carved with `compute_shielded_unshield_fee` (the base fee PLUS the flat storage
    // cost of the single `AddBalanceToAddress` write crediting the net to the output address), so
    // reserve against `ShieldedFeeKind::Unshield` — reserving the base fee here would under-fund the
    // address-write cost and the builder would reject the spend (and the `fee_used == exact_fee`
    // debug assert below would fire).
    let (selected_notes, total_input, exact_fee, mut reservation) = reserve_unspent_notes(
        sdk,
        store,
        id,
        amount,
        // 1 shielded output (the change note); the 2-action padding comes from the
        // shared layout rule.
        1,
        ShieldedFeeKind::Unshield,
        ChangeRequirement::Optional,
    )
    .await?;

    info!(
        account,
        credits = amount,
        fee = exact_fee,
        inputs = selected_notes.len(),
        total_input,
        "Unshield"
    );

    // From here on an error before the spend's record is saved releases the
    // reservation taken by `reserve_unspent_notes`; once it is saved, only a
    // transition that never left the wallet does (`settle_failed_spend`).
    //
    // `pending_entry` is recorded once the transition is built (so we
    // have its output cmxs) and flipped to Confirmed / Failed in the
    // outer match. It lives outside the async block so the flip can see
    // it. A build failure leaves it `None` and records nothing.
    let mut pending_entry = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        // Capture the recorded anchor before the builder consumes it, so a
        // broadcast-accepted-but-unconfirmed spend can be auto-released once
        // this anchor is pruned from Platform's recorded set.
        let anchor_bytes = anchor.to_bytes();

        // The builder computes and returns the fee authoritatively; `exact_fee` (== the
        // minimum) was already used above for note reservation.
        //
        // Build + prove runs under the pre-broadcast panic guard (see
        // `catch_pre_broadcast_panic`): a prover panic here is provably pre-broadcast, so it
        // must become a DEFINITIVE error the outer failure arm releases the reservation on —
        // not an unwind past these match arms into the FFI guard's ambiguous contract, which
        // strands the anchor-less reservation for the rest of the process lifetime.
        let (state_transition, fee_used) = catch_pre_broadcast_panic("unshield", || {
            build_unshield_transition(
                spends,
                *to_address,
                amount,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                [0u8; 36],
                sdk.version(),
            )
        })?
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        // The builder's fee and the wallet's reserved `exact_fee` both come from
        // compute_shielded_unshield_fee with the same action count; lock that they agree.
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved unshield fee"
        );

        // Live activity: a confirmed Unshield is `direction out`,
        // counterparty = the 21-byte serialized PlatformAddress, exact
        // fee = the builder's metered fee.
        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind: ShieldedActivityKind::Unshield,
                direction: ShieldedDirection::Out,
                amount,
                fee: Some(fee_used),
                counterparty: Some(to_address.to_bytes()),
                memo: None,
                actions: shielded_actions(&state_transition),
                spent_notes: &selected_notes,
            },
        )
        .await;

        trace!("Unshield: state transition built, saving and broadcasting...");
        broadcast_shielded_spend_with_redrive(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &state_transition,
            "unshield",
            SpendResultWait::ExecutionProved,
        )
        .await
    }
    .await;

    match result {
        Ok(_) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            // Broadcast already succeeded; spent-state bookkeeping is
            // best-effort. Surfacing a local write failure as a send
            // failure here would invite duplicate retries — the next
            // note scan reconciles any drift (scan-based spend
            // detection re-marks the note from its on-chain nullifier).
            //
            // No double-spend follows from this downgrade: the
            // authoritative no-reuse guarantee is the on-chain nullifier
            // set, not this local mark. Worst case, before the next note
            // scan runs the note is re-selected and a second spend is
            // built + proven, then rejected at broadcast with a
            // nullifier-already-used error — wasted ~30 s proof, never
            // fund loss. (A restart restores the reservation only while
            // the spend's saved record is still there; the on-chain set
            // protects either way.)
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "Unshield broadcast succeeded but local spent-state update failed; \
                     will heal on next sync"
                );
            }
            info!(account, credits = amount, "Unshield broadcast succeeded");
            Ok(())
        }
        // A spend that may have reached a node keeps its reservation, its
        // saved record and its `Pending` activity row: a later scan that
        // finds the spend marks the notes spent and flips the row to
        // Confirmed (its id matches), and the saved record restores the
        // reservation after a restart. Releasing the notes would invite
        // re-selecting notes whose nullifiers may already be consumed on
        // chain. Only a spend that failed before its record was saved, or
        // whose transition never left the wallet, releases them.
        Err(failure) => Err(settle_failed_spend(
            store,
            persister,
            wallet_id,
            id,
            &pending_entry,
            reservation,
            "unshield",
            failure,
        )
        .await),
    }
}

// -------------------------------------------------------------------------
// Transfer: shielded pool -> shielded pool (Type 16)
// -------------------------------------------------------------------------

/// Transfer funds privately from `account`'s shielded notes to
/// another Orchard payment address.
#[allow(clippy::too_many_arguments)]
pub async fn transfer<S: ShieldedStore, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    to_address: &PaymentAddress,
    amount: u64,
    memo: [u8; 36],
    prover: &P,
) -> Result<(), PlatformWalletError> {
    let recipient_addr = payment_address_to_orchard(to_address)?;
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);

    // ShieldedTransfer is carved with the base `compute_minimum_shielded_fee`, so reserve
    // against `ShieldedFeeKind::Base`.
    let (selected_notes, total_input, exact_fee, mut reservation) = reserve_unspent_notes(
        sdk,
        store,
        id,
        amount,
        2,
        ShieldedFeeKind::Base,
        ChangeRequirement::Optional,
    )
    .await?;

    info!(
        account,
        credits = amount,
        fee = exact_fee,
        inputs = selected_notes.len(),
        total_input,
        "Shielded transfer"
    );

    let mut pending_entry = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        // Capture the recorded anchor before the builder consumes it, so a
        // broadcast-accepted-but-unconfirmed spend can be auto-released once
        // this anchor is pruned from Platform's recorded set.
        let anchor_bytes = anchor.to_bytes();

        // The builder computes and returns the fee authoritatively; `exact_fee` (== the
        // minimum) was already used above for note reservation.
        //
        // Build + prove runs under the pre-broadcast panic guard, exactly as in `transfer_multi`
        // (see `catch_pre_broadcast_panic`): a prover panic must release the reservation rather
        // than unwind past these match arms into the FFI guard's ambiguous contract.
        let (state_transition, fee_used) = catch_pre_broadcast_panic("transfer", || {
            build_shielded_transfer_transition(
                spends,
                &recipient_addr,
                amount,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                memo,
                sdk.version(),
            )
        })?
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        // The builder's fee and the wallet's reserved `exact_fee` both come from
        // compute_minimum_shielded_fee with the same action count; lock that they agree.
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved minimum fee"
        );

        // Live activity: a confirmed transfer is a `Sent` (direction out),
        // counterparty = the recipient's 43-byte raw Orchard address, memo
        // attached when non-zero.
        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind: ShieldedActivityKind::Sent,
                direction: ShieldedDirection::Out,
                amount,
                fee: Some(fee_used),
                counterparty: Some(to_address.to_raw_address_bytes().to_vec()),
                memo: non_zero_memo(&memo),
                actions: shielded_actions(&state_transition),
                spent_notes: &selected_notes,
            },
        )
        .await;

        trace!("Shielded transfer: state transition built, saving and broadcasting...");
        broadcast_shielded_spend_with_redrive(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &state_transition,
            "transfer",
            SpendResultWait::ExecutionProved,
        )
        .await
    }
    .await;

    match result {
        Ok(_) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            // Best-effort post-broadcast bookkeeping (see unshield).
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "Shielded transfer broadcast succeeded but local spent-state update \
                     failed; will heal on next sync"
                );
            }
            info!(
                account,
                credits = amount,
                "Shielded transfer broadcast succeeded"
            );
            Ok(())
        }
        // See `unshield`'s outer match.
        Err(failure) => Err(settle_failed_spend(
            store,
            persister,
            wallet_id,
            id,
            &pending_entry,
            reservation,
            "transfer",
            failure,
        )
        .await),
    }
}

/// Transfer funds privately from `account`'s shielded notes to
/// SEVERAL Orchard outputs in one atomic transition (Type 16).
///
/// Multi-output sibling of [`transfer`]. Each `(address, amount)` pair becomes its own note —
/// including when several pairs name the SAME address, which is how one transition funds an
/// address with more than one note.
///
/// `memo` is attached to every recipient note (the change note always carries the empty memo).
///
/// # Fee sizing
///
/// The bundle publishes `max(spends, recipients + 1, 2)` actions and a `ShieldedTransfer`'s
/// `value_balance` must equal `compute_minimum_shielded_fee(actions.len())` EXACTLY. Note
/// selection therefore reserves against `recipients.len() + 1` outputs — the same floor the
/// builder sizes its fee from — so the reserved and carved fees cannot diverge.
#[allow(clippy::too_many_arguments)]
pub async fn transfer_multi<S: ShieldedStore, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    outputs: &[(PaymentAddress, u64)],
    memo: [u8; 36],
    prover: &P,
) -> Result<(), PlatformWalletError> {
    if outputs.is_empty() {
        return Err(PlatformWalletError::ShieldedBuildError(
            "a multi-output shielded transfer needs at least one recipient output".to_string(),
        ));
    }

    let builder_outputs: Vec<ShieldedTransferOutput> = outputs
        .iter()
        .map(|(addr, amount)| {
            Ok(ShieldedTransferOutput {
                recipient: payment_address_to_orchard(addr)?,
                amount: *amount,
                memo,
            })
        })
        .collect::<Result<_, PlatformWalletError>>()?;

    let total_amount = outputs
        .iter()
        .try_fold(0u64, |acc, (_, amount)| acc.checked_add(*amount))
        .ok_or_else(|| {
            PlatformWalletError::ShieldedBuildError(
                "multi-output shielded transfer amounts overflow u64".to_string(),
            )
        })?;

    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);

    // Reserve against the SAME output count the builder sizes its fee from: every recipient
    // output plus the unconditional change output.
    //
    // That change output is unconditional and must carry a positive value, so the builder
    // rejects `total_input == total_amount + fee`. Selection must therefore demand STRICTLY
    // more, or a wallet holding e.g. `[total_amount + fee, 1]` would have the exact-coverage
    // note reserved on its own and the build would fail despite the balance being sufficient.
    let num_outputs = builder_outputs.len() + 1;
    let (selected_notes, total_input, exact_fee, mut reservation) = reserve_unspent_notes(
        sdk,
        store,
        id,
        total_amount,
        num_outputs,
        ShieldedFeeKind::Base,
        ChangeRequirement::StrictlyPositive,
    )
    .await?;

    info!(
        account,
        credits = total_amount,
        note_outputs = builder_outputs.len(),
        fee = exact_fee,
        inputs = selected_notes.len(),
        total_input,
        "Shielded multi-output transfer"
    );

    let mut pending_entry = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        let anchor_bytes = anchor.to_bytes();

        // Build + prove under a pre-broadcast panic guard: a prover panic here must become a
        // DEFINITIVE error (broadcast provably never happened) so the outer failure arm
        // releases the reservation — otherwise the panic unwinds to the FFI boundary, which
        // keeps the (anchor-less) reservation under its ambiguous-outcome contract and no
        // sync can ever free the notes (`stale_pending_spends` skips unarmed reservations).
        let (state_transition, fee_used) = catch_pre_broadcast_panic("transfer_multi", || {
            build_shielded_transfer_transition_multi(
                spends,
                &builder_outputs,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                sdk.version(),
            )
        })?
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved minimum fee"
        );

        // One activity row for the whole transition.
        //
        // Kind / amount / counterparty come from the shared
        // `live_transfer_multi_row` rule (activity.rs): partition the
        // requested outputs into EXTERNAL payments vs WALLET-OWNED receipts
        // with this account's `IncomingViewingKey::diversifier_index` — the
        // same ownership test `coordinator::is_own_orchard_recipient` feeds
        // the cold-restore deriver — then derive the row from the external
        // subset only, with the counterparty through the shared
        // `unanimous_bytes` rule over the canonical 43-byte raw address
        // encodings. The `live_and_restored_agree_*` parity tests
        // (activity.rs) run THIS function against the deriver, so the two
        // paths cannot drift apart silently (#4312 review finding
        // 379da4cc0ad0).
        //
        // The public multi-transfer API accepts ANY valid Orchard address,
        // including this account's own diversified ones. Without the split,
        // 10 credits to an external address plus 20 to an own address
        // recorded live as a 30-credit `Sent` with no counterparty but
        // restored as a 10-credit `Sent` to the external address, and an
        // all-own output set recorded live as `Sent` but restored as a
        // shielded spend.
        let (kind, amount, counterparty) =
            live_transfer_multi_row(&views.incoming_viewing_key, outputs, fee_used);

        // No memo is claimed on a row that paid no one: the deriver's
        // own-only arms record none, and the only credits that left the
        // pool are the fee.
        let entry_memo = match kind {
            ShieldedActivityKind::Sent => non_zero_memo(&memo),
            _ => None,
        };

        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind,
                direction: ShieldedDirection::Out,
                amount,
                fee: Some(fee_used),
                counterparty,
                memo: entry_memo,
                actions: shielded_actions(&state_transition),
                spent_notes: &selected_notes,
            },
        )
        .await;

        trace!("Shielded multi-output transfer: state transition built, broadcasting...");
        broadcast_shielded_spend_with_redrive(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &state_transition,
            "transfer_multi",
            SpendResultWait::ExecutionProved,
        )
        .await
    }
    .await;

    match result {
        Ok(_) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "Shielded multi-output transfer broadcast succeeded but local spent-state \
                     update failed; will heal on next sync"
                );
            }
            info!(
                account,
                credits = total_amount,
                "Shielded multi-output transfer broadcast succeeded"
            );
            Ok(())
        }
        // See `unshield`'s outer match.
        Err(failure) => Err(settle_failed_spend(
            store,
            persister,
            wallet_id,
            id,
            &pending_entry,
            reservation,
            "transfer_multi",
            failure,
        )
        .await),
    }
}

// -------------------------------------------------------------------------
// Withdraw: shielded pool -> Core L1 address (Type 19)
// -------------------------------------------------------------------------

/// Withdraw funds from `account`'s shielded notes to a Core L1 address.
#[allow(clippy::too_many_arguments)]
pub async fn withdraw<S: ShieldedStore, P: OrchardProver>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    to_address: &dashcore::Address,
    amount: u64,
    core_fee_per_byte: u32,
    prover: &P,
) -> Result<(), PlatformWalletError> {
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);
    let output_script = CoreScript::from_bytes(to_address.script_pubkey().to_bytes());

    // Reserve against the 2-action floor: Orchard's BundleType::DEFAULT pads single-spend
    // bundles to 2 actions, and the builder prices the fee at spends.len().max(2). Reserving
    // for 1 would under-fee a single-note transition and the builder would reject it locally.
    // ShieldedWithdrawal is carved with `compute_shielded_withdrawal_fee` (the base fee PLUS the
    // flat Core withdrawal-document storage cost), so reserve against
    // `ShieldedFeeKind::Withdrawal` — reserving the base fee here would under-fund the document
    // cost and the builder would reject the spend (and the `fee_used == exact_fee` debug assert
    // below would fire).
    let (selected_notes, total_input, exact_fee, mut reservation) = reserve_unspent_notes(
        sdk,
        store,
        id,
        amount,
        1,
        ShieldedFeeKind::Withdrawal,
        ChangeRequirement::Optional,
    )
    .await?;

    info!(
        account,
        credits = amount,
        fee = exact_fee,
        inputs = selected_notes.len(),
        total_input,
        "Shielded withdrawal"
    );

    // Capture the Core output script bytes for the activity counterparty
    // (the same bytes that fed `output_script` above) before the builder
    // consumes `output_script`.
    let counterparty_script = to_address.script_pubkey().to_bytes();

    let mut pending_entry = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        // Capture the recorded anchor before the builder consumes it, so a
        // broadcast-accepted-but-unconfirmed spend can be auto-released once
        // this anchor is pruned from Platform's recorded set.
        let anchor_bytes = anchor.to_bytes();

        // The builder computes and returns the fee authoritatively; `exact_fee` (== the
        // minimum) was already used above for note reservation.
        //
        // Build + prove runs under the pre-broadcast panic guard (see
        // `catch_pre_broadcast_panic`): a prover panic must release the reservation rather than
        // unwind past these match arms into the FFI guard's ambiguous contract.
        let (state_transition, fee_used) = catch_pre_broadcast_panic("withdraw", || {
            build_shielded_withdrawal_transition(
                spends,
                amount,
                output_script,
                core_fee_per_byte,
                // Consensus pins shielded-withdrawal pooling to Never (validate_structure).
                Pooling::Never,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                [0u8; 36],
                sdk.version(),
            )
        })?
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
        // The builder's fee and the wallet's reserved `exact_fee` both come from
        // compute_shielded_withdrawal_fee with the same action count; lock that they agree.
        debug_assert_eq!(
            fee_used, exact_fee,
            "builder fee must match the reserved withdrawal fee"
        );

        // Live activity: a Withdrawal (direction out), counterparty = the
        // Core output script bytes, exact metered fee.
        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind: ShieldedActivityKind::Withdrawal,
                direction: ShieldedDirection::Out,
                amount,
                fee: Some(fee_used),
                counterparty: Some(counterparty_script.clone()),
                memo: None,
                actions: shielded_actions(&state_transition),
                spent_notes: &selected_notes,
            },
        )
        .await;

        trace!("Shielded withdrawal: state transition built, saving and broadcasting...");
        broadcast_shielded_spend_with_redrive(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &state_transition,
            "withdraw",
            SpendResultWait::ExecutionProved,
        )
        .await
    }
    .await;

    match result {
        Ok(_) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            // Best-effort post-broadcast bookkeeping (see unshield).
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "Shielded withdrawal broadcast succeeded but local spent-state update \
                     failed; will heal on next sync"
                );
            }
            info!(
                account,
                credits = amount,
                "Shielded withdrawal broadcast succeeded"
            );
            Ok(())
        }
        // See `unshield`'s outer match.
        Err(failure) => Err(settle_failed_spend(
            store,
            persister,
            wallet_id,
            id,
            &pending_entry,
            reservation,
            "withdraw",
            failure,
        )
        .await),
    }
}

// -------------------------------------------------------------------------
// IdentityCreateFromShieldedPool: shielded pool -> brand-new identity (Type 20)
// -------------------------------------------------------------------------

/// Create a brand-new Platform identity funded directly from `account`'s shielded notes.
///
/// Spends notes covering `denomination` (a member of the versioned exit-denomination set); the whole
/// denomination leaves the pool (`value_balance == denomination` EXACTLY, the ShieldedTransfer
/// exact-equality model) and the metered fee is taken FROM the denomination at execution, so the new
/// identity is created holding `denomination - total_fee`. Any spent value above the denomination
/// re-enters the pool as a single change note to `account`'s default Orchard address.
///
/// `public_keys` is the new identity's key set (each entry is the `IdentityPublicKey` and its
/// `IdentityPublicKeyInCreation` form); `identity_signer` produces each key's proof-of-possession
/// signature over the transition's signable bytes. Authorization is 100% the Orchard proof +
/// per-action spend-auth signatures + binding signature (which commits the derived id + denomination
/// + full key set) + the per-key PoP — there is NO platform identity signature.
///
/// Returns the new identity's id (`double_sha256(sorted nullifiers)`, derived deterministically
/// from the spent notes' nullifiers) together with the proof-verified [`Identity`] returned by the
/// SDK broadcast. The caller registers that `Identity` in its local `IdentityManager` so the host
/// persister emits the row, mirroring the address-funded registration path.
#[allow(clippy::too_many_arguments)]
pub async fn identity_create_from_shielded_pool<S, P, IS>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    keys: &OrchardKeySet,
    account: u32,
    public_keys: Vec<(IdentityPublicKey, IdentityPublicKeyInCreation)>,
    denomination: u64,
    send_to_address_on_creation_failure: PlatformAddress,
    identity_signer: &IS,
    prover: &P,
) -> Result<(Identifier, Identity), PlatformWalletError>
where
    S: ShieldedStore,
    P: OrchardProver,
    IS: Signer<IdentityPublicKey>,
{
    if public_keys.is_empty() {
        return Err(PlatformWalletError::ShieldedBuildError(
            "identity-create-from-shielded-pool requires at least one public key".to_string(),
        ));
    }
    let views = keys.viewing_keys();
    let change_addr = default_orchard_address(&views)?;
    let id = SubwalletId::new(wallet_id, account);
    let num_keys = public_keys.len();

    // Exact-equality model: reserve notes covering the denomination itself (NOT denomination + fee
    // — the fee is metered FROM the denomination at execution). The reservation also gates on
    // `denomination > predicted_fee` so the new identity can't be created with a non-positive
    // balance. Orchard's BundleType::DEFAULT pads single-spend bundles to a 2-action floor.
    let (selected_notes, total_input, predicted_fee, mut reservation) =
        reserve_unspent_notes_for_denomination(sdk, store, id, denomination, 1, num_keys).await?;

    info!(
        account,
        denomination,
        predicted_fee,
        inputs = selected_notes.len(),
        total_input,
        keys = num_keys,
        "IdentityCreateFromShieldedPool"
    );

    // Snapshot the submitted `IdentityPublicKey` halves keyed by their `KeyID` BEFORE the build
    // consumes `public_keys`. This is the canonical record of the key set the transition commits to
    // (the binding signature covers it), so it's the defensive fallback if the proof-verified
    // identity comes back with an empty `public_keys()` map — same pattern register_from_addresses
    // uses for its address-funded `put_*` stub.
    let submitted_public_keys: BTreeMap<u32, IdentityPublicKey> = public_keys
        .iter()
        .map(|(key, _)| (key.id(), key.clone()))
        .collect();

    // From here on an error before the create's record is saved releases the reservation taken
    // above; once it is saved, only a transition that never left the wallet does
    // (`settle_failed_spend`).
    //
    // `pending_entry` is recorded once the bundle is built (so we know the
    // identity id + output cmxs) and flipped to Confirmed / Failed in the
    // outer match; it lives here so the flip can see it. So does
    // `derived_identity_id`, which an unconfirmed create reports.
    let mut pending_entry = None;
    let mut derived_identity_id = None;
    let result = async {
        let (spends, anchor, anchor_height) =
            extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
        // Capture the recorded anchor before the builder consumes it, so a
        // broadcast-accepted-but-unconfirmed create can be auto-released once
        // this anchor is pruned from Platform's recorded set.
        let anchor_bytes = anchor.to_bytes();

        // Build + prove runs under the ASYNC pre-broadcast panic guard (see
        // `catch_pre_broadcast_panic_async`). This builder is a future — it awaits the per-key
        // proof-of-possession signer, which on a host build is a foreign callback — but it still
        // runs strictly before the broadcast below, so a panic on any of its polls is provably
        // pre-broadcast and must become a DEFINITIVE error the outer failure arm releases the
        // reservation on, rather than unwinding past these match arms into the FFI guard's
        // ambiguous contract (which strands the anchor-less reservation for the process
        // lifetime).
        let build = catch_pre_broadcast_panic_async(
            "identity_create",
            build_identity_create_from_shielded_pool_transition(
                public_keys,
                denomination,
                send_to_address_on_creation_failure,
                spends,
                &change_addr,
                &keys.full_viewing_key,
                &keys.spend_auth_key,
                anchor,
                prover,
                identity_signer,
                [0u8; 36],
                sdk.version(),
            ),
        )
        .await?
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;

        let identity_id = build.identity_id;
        derived_identity_id = Some(identity_id);

        // Live activity: IdentityCreate carries the created identity id.
        // The display amount is the denomination LESS the metered fee (the
        // credits the new identity is created holding); the exact fee is the
        // predicted fee reserved above (the builder meters the same value).
        // The output cmxs are taken from the bundle the transition is built
        // from (the change note re-entering the pool, if any).
        pending_entry = record_pending_activity(
            store,
            persister,
            wallet_id,
            id,
            &views,
            LiveEntryParams {
                kind: ShieldedActivityKind::IdentityCreate {
                    identity_id: identity_id.to_buffer(),
                },
                direction: ShieldedDirection::Out,
                amount: denomination.saturating_sub(predicted_fee),
                fee: Some(predicted_fee),
                counterparty: Some(identity_id.to_buffer().to_vec()),
                memo: None,
                actions: &build.bundle.actions,
                spent_notes: &selected_notes,
            },
        )
        .await;

        // The transition is built once from the PoP-signed keys + bundle params (preserving the
        // per-key signatures); the binding signature already committed `identity_id`, the
        // denomination, and the full key set. It is built before its record is saved, which
        // keeps these exact bytes.
        let st = sdk
            .identity_create_from_shielded_pool_transition(
                build.public_keys,
                denomination,
                send_to_address_on_creation_failure,
                build.bundle,
            )
            .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;

        trace!("IdentityCreateFromShieldedPool: built, saving and broadcasting via SDK...");
        broadcast_identity_create_with_redrive(
            sdk,
            store,
            id,
            &pending_entry,
            anchor_bytes,
            anchor_height,
            &selected_notes,
            &mut reservation,
            &st,
            identity_id,
            submitted_public_keys,
        )
        .await
    }
    .await;

    match result {
        Ok((identity_id, identity)) => {
            // It executed: the outcome owns the notes, which it marks spent.
            reservation.hand_over();
            record_activity_status(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                ShieldedActivityStatus::Confirmed,
                None,
            )
            .await;
            // Best-effort post-broadcast bookkeeping (see `unshield`): mark the spent notes so the
            // local balance reflects the exit immediately; any drift heals on the next nullifier
            // sync. The on-chain nullifier set — not this local mark — is the authoritative
            // no-reuse guarantee.
            if let Err(e) = finalize_pending(store, persister, wallet_id, id, &selected_notes).await
            {
                warn!(
                    account,
                    error = %e,
                    "IdentityCreateFromShieldedPool broadcast succeeded but local spent-state \
                     update failed; will heal on next sync"
                );
            }
            info!(
                account,
                denomination,
                identity_id = %identity_id,
                "IdentityCreateFromShieldedPool broadcast succeeded"
            );
            Ok((identity_id, identity))
        }
        // A create that may have reached a node keeps its reservation, its saved record and its
        // `Pending` activity row (see `unshield`'s outer match), and reports the derived id: the
        // identity may already exist on chain, so the caller holds its registration slot.
        Err(failure) => {
            let error = settle_failed_spend(
                store,
                persister,
                wallet_id,
                id,
                &pending_entry,
                reservation,
                IDENTITY_CREATE_OPERATION,
                failure,
            )
            .await;
            Err(identity_create_failure(error, derived_identity_id))
        }
    }
}

/// The error an identity create reports for `error`, what [`settle_failed_spend`] settled its
/// failure as. A create settled as unconfirmed carries the derived `identity_id`
/// ([`PlatformWalletError::ShieldedBroadcastUnconfirmed`]): the identity may already exist on
/// chain, so the caller must hold its registration slot rather than read a failed create.
fn identity_create_failure(
    error: PlatformWalletError,
    identity_id: Option<Identifier>,
) -> PlatformWalletError {
    match (error, identity_id) {
        (PlatformWalletError::ShieldedSpendUnconfirmed { reason, .. }, Some(identity_id)) => {
            PlatformWalletError::ShieldedBroadcastUnconfirmed {
                identity_id,
                reason,
            }
        }
        (error, _) => error,
    }
}

/// Save the built identity create `st`'s recovery record durably, then
/// broadcast it and wait for its result ([`save_spend_then_broadcast`],
/// [`broadcast_identity_create`]): the identity create's counterpart of
/// [`broadcast_shielded_spend_with_redrive_with_metadata`].
#[allow(clippy::too_many_arguments)]
async fn broadcast_identity_create_with_redrive<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    anchor: [u8; 32],
    anchor_height: u64,
    notes: &[ShieldedNote],
    reservation: &mut NoteReservation<S>,
    st: &StateTransition,
    identity_id: Identifier,
    submitted_public_keys: BTreeMap<u32, IdentityPublicKey>,
) -> Result<(Identifier, Identity), SpendFailure> {
    save_spend_then_broadcast(
        store,
        id,
        pending_entry,
        anchor,
        anchor_height,
        notes,
        reservation,
        st,
        IDENTITY_CREATE_OPERATION,
        || broadcast_identity_create(sdk, st, identity_id, submitted_public_keys),
    )
    .await
}

/// What an identity create is named in the errors its record's save and its
/// settlement report.
const IDENTITY_CREATE_OPERATION: &str = "identity create from shielded pool";

/// Broadcast the built identity create `st`, whose record is already saved
/// ([`save_spend_then_broadcast`]), and wait for its result.
///
/// The broadcast and the result wait are staged separately so the two failure shapes can be
/// told apart:
///   - a transition that never left the wallet ([`BroadcastDelivery::NeverDelivered`]: refused
///     before anything was sent) can never execute, so its record is retired and its notes are
///     released ([`SpendFailure::never_delivered`]);
///   - every other failure may have delivered it. It falls through to the result wait, and a
///     wait that fails falls back to fetching the identity by its pre-derived id before the
///     create is reported unconfirmed ([`PlatformWalletError::ShieldedBroadcastUnconfirmed`]),
///     its notes and record kept. That includes a `StateTransitionBroadcastError` with a
///     consensus `cause`: the endpoint that reports it is unauthenticated, no proof or quorum
///     signature backs it, and one that kept or forwarded the transition can answer with any
///     verdict. It also includes cause-less ones, as DAPI encodes its own wait-side failures
///     (timeouts, internal errors) that way.
///
/// Nothing here reports a failed create once a dispatch may have reached a node: a create
/// reported failed while it may still execute is the orphaned-identity hazard this staging
/// exists to avoid.
async fn broadcast_identity_create(
    sdk: &Arc<dash_sdk::Sdk>,
    st: &StateTransition,
    identity_id: Identifier,
    submitted_public_keys: BTreeMap<u32, IdentityPublicKey>,
) -> Result<(Identifier, Identity), SpendFailure> {
    let unconfirmed = |reason: String| {
        SpendFailure::from(PlatformWalletError::ShieldedBroadcastUnconfirmed {
            identity_id,
            reason,
        })
    };

    match broadcast_tracking_delivery(sdk, st).await {
        BroadcastDelivery::Admitted => {}
        BroadcastDelivery::NeverDelivered(e) => {
            return Err(SpendFailure {
                error: definite_broadcast_failure(&e),
                never_delivered: true,
            });
        }
        BroadcastDelivery::MaybeDelivered(e) => {
            warn!(
                derived_id = %identity_id,
                error = %e,
                "IdentityCreateFromShieldedPool: broadcast may have been delivered; falling \
                 through to the result wait"
            );
        }
    }

    // An identity-create-from-shielded-pool proof authenticates the spent
    // nullifiers and resulting identity as a snapshot; it cannot bind the
    // complete Orchard request, so accept the affected-state outcome.
    let proof_result = match st
        .wait_for_affected_state::<StateTransitionProofResult>(sdk, None)
        .await
    {
        Ok(result) => result,
        Err(wait_err) => {
            warn!(
                derived_id = %identity_id,
                error = %wait_err,
                "IdentityCreateFromShieldedPool: broadcast accepted but result confirmation \
                 failed; falling back to fetching the identity by its derived id"
            );
            // Fetch the identity directly. The id is committed in the sighash and derived
            // deterministically from the spent nullifiers, so if the transition landed the row
            // is queryable. A freshly-included identity can take a moment to index on the DAPI
            // node we hit, so retry a few times with a short, fixed backoff before concluding
            // it's truly absent: long enough to ride out routine indexing/replica lag, short
            // enough not to wedge the caller's UI for minutes.
            return match fetch_identity_with_retries(sdk, identity_id).await {
                Some(mut identity) => {
                    info!(
                        derived_id = %identity_id,
                        "IdentityCreateFromShieldedPool: result confirmation failed but the \
                         identity was found on chain by its derived id; treating as success"
                    );
                    // Same defensive empty-`public_keys` fill as the proven-result path below,
                    // so downstream auth-key checks see the committed key set immediately.
                    if identity.public_keys().is_empty() {
                        identity.set_public_keys(submitted_public_keys);
                    }
                    Ok((identity.id(), identity))
                }
                // The saved record resolves it: the sync-time pass re-checks the nullifiers,
                // then re-broadcasts this byte-identical transition up to
                // MAX_REDRIVE_ATTEMPTS times.
                None => Err(unconfirmed(wait_err.to_string())),
            };
        }
    };

    // Pull the verified `Identity` out of the proof result. The expected variant is
    // `VerifiedIdentityWithShieldedNullifiers`; if drive-abci ever returns a different one the
    // broadcast still SUCCEEDED, so we don't turn it into a failed create: we synthesize the
    // identity from the derived id + submitted keys (the binding signature committed both) and
    // warn, so the local row is still created.
    let identity = match proof_result {
        StateTransitionProofResult::VerifiedIdentityWithShieldedNullifiers(
            mut identity,
            _nullifiers,
        ) => {
            // The proof-verified id is authoritative: it's recomputed from the proven nullifier
            // set, while `identity_id` was derived pre-broadcast. They should match (the derived
            // id is committed in the sighash), but trust the verified one.
            if identity.id() != identity_id {
                warn!(
                    derived_id = %identity_id,
                    verified_id = %identity.id(),
                    "IdentityCreateFromShieldedPool: derived id differs from proof-verified id; \
                     using the proof-verified id"
                );
            }
            // Defensive: a proof result can hand back an identity whose `public_keys` map is
            // empty. Fill it from the submitted set so downstream auth-key checks see the keys
            // immediately without waiting for the next identity-fetch round (the transition
            // committed exactly these keys, so id reproducibility is preserved).
            if identity.public_keys().is_empty() {
                identity.set_public_keys(submitted_public_keys);
            }
            identity
        }
        other => {
            warn!(
                derived_id = %identity_id,
                result = %other,
                "IdentityCreateFromShieldedPool: unexpected proof-result variant; synthesizing \
                 the identity from the derived id + submitted keys so the local row still lands"
            );
            // The create executed. An identity that cannot be synthesized leaves it
            // unconfirmed, its notes and record kept for the next sync, never failed.
            Identity::new_with_id_and_keys(identity_id, submitted_public_keys, sdk.version())
                .map_err(|e| {
                    unconfirmed(format!(
                        "its result was proved, but the identity could not be read from it: {e}"
                    ))
                })?
        }
    };

    Ok((identity.id(), identity))
}

/// Number of times [`identity_create_from_shielded_pool`] re-fetches the new identity by its
/// derived id after a post-broadcast result-confirmation failure, before declaring the broadcast
/// unconfirmed.
const IDENTITY_CREATE_FETCH_RETRIES: usize = 4;

/// Fixed backoff between identity fetch attempts. Four attempts ~3 s apart (~9 s of fetch window
/// total) is enough to ride out routine DAPI indexing / replica lag for a freshly-included identity
/// without wedging the caller's UI for minutes.
const IDENTITY_CREATE_FETCH_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// Fetch an identity by id with a few fixed-interval retries.
///
/// Used only on the ambiguous post-broadcast path: the result-proof fetch failed, so we don't know
/// whether the transition executed. The identity id is derived deterministically from the spent
/// notes' nullifiers and committed in the transition sighash, so a successful fetch is positive
/// proof the transition landed. Returns `Some(identity)` on the first hit, or `None` if every
/// attempt comes back empty or errors (transport hiccup, not-yet-indexed, …) — the caller then
/// surfaces `ShieldedBroadcastUnconfirmed` rather than a hard failure.
async fn fetch_identity_with_retries(
    sdk: &Arc<dash_sdk::Sdk>,
    identity_id: Identifier,
) -> Option<Identity> {
    use dash_sdk::platform::Fetch;

    for attempt in 0..IDENTITY_CREATE_FETCH_RETRIES {
        match Identity::fetch(sdk, identity_id).await {
            Ok(Some(identity)) => return Some(identity),
            Ok(None) => {
                trace!(
                    %identity_id,
                    attempt,
                    "IdentityCreateFromShieldedPool confirmation fetch: not found yet"
                );
            }
            Err(e) => {
                trace!(
                    %identity_id,
                    attempt,
                    error = %e,
                    "IdentityCreateFromShieldedPool confirmation fetch errored; will retry"
                );
            }
        }
        // Skip the trailing sleep after the final attempt — nothing follows it.
        if attempt + 1 < IDENTITY_CREATE_FETCH_RETRIES {
            tokio::time::sleep(IDENTITY_CREATE_FETCH_RETRY_DELAY).await;
        }
    }
    None
}

// -------------------------------------------------------------------------
// IdentityCreateFromShieldedPool from a ONE-TIME Orchard key (Type 20, L2
// invitations — the claim side)
// -------------------------------------------------------------------------

/// Create a brand-new Platform identity funded from a ONE-TIME Orchard spending
/// key (the L2-invitation *claim* side).
///
/// Unlike [`identity_create_from_shielded_pool`], the spend authority is NOT the
/// wallet's own [`OrchardKeySet`]; it is a foreign `one_time_sk` — the single-use
/// Orchard spending key an invitation was funded to. The op:
/// 1. derives the full-viewing / incoming-viewing / spend-authorizing keys from
///    `one_time_sk`,
/// 2. transiently scans the network for the note(s) that key owns (they are not
///    tracked in any subwallet store — see [`super::sync::scan_notes_for_foreign_key`]),
/// 3. selects notes covering exactly `denomination` (the exact-equality model —
///    the fee is metered FROM the denomination) and gates on
///    `denomination > predicted_fee`,
/// 4. witnesses the selected notes against a Platform-recorded anchor from the
///    shared (fully-marked) commitment tree — the SAME anchor probe the
///    pool-funded op uses (so a wallet that hasn't synced past the funding
///    position gets the retryable [`PlatformWalletError::ShieldedMerkleWitnessUnavailable`]),
/// 5. feeds the key-agnostic Type-20 builder with the one-time key's fvk/ask, and
/// 6. broadcasts + waits with the same fetch-by-derived-id fallback.
///
/// The whole denomination leaves the pool; any spent value above it re-enters as
/// a single change note to `change_address` (the claimer's OWN default Orchard
/// address — over-funding is expected to be zero for a one-time invitation key,
/// but is handled). There is NO wallet-side note reservation to take or release:
/// the spent notes belong to the foreign key, not to any subwallet, so an
/// unconfirmed broadcast simply leaves the on-chain nullifiers as the
/// authoritative no-reuse guarantee.
///
/// `funding_birth_height` is an advisory hint only (see
/// [`super::sync::scan_notes_for_foreign_key`] — the tree has no height→position
/// oracle, so it cannot seed the scan start today; repeated attempts are
/// bounded by the scan's coordinator-owned resume checkpoint instead).
///
/// Returns the new identity's id, the proof-verified [`Identity`], and the
/// claim's retained recovery record. The caller registers that identity in its
/// local `IdentityManager` and, once that registration is DURABLE, passes the
/// record to [`acknowledge_one_time_claim_registration`] — which is what
/// finally drops it. Until then the record survives the native return, so a
/// caller that dies, cannot find its wallet entry, or fails to persist the
/// identity leaves a retry able to recover the exact identity id rather than
/// hitting the terminal [`PlatformWalletError::ShieldedInviteAlreadyClaimed`]
/// (#4313 review finding 325ce9fa8f84).
///
/// `detached` is the claimer wallet's irreversible removal flag. It is re-read
/// once this claim holds store admission, so a claim that begins after a wallet
/// removal has committed is refused with
/// [`PlatformWalletError::WalletNotFound`] before anything is scanned, built or
/// broadcast (#4313 review finding 4a2c679745bb).
#[allow(clippy::too_many_arguments)]
pub async fn identity_create_from_one_time_key<S, P, IS>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    // Coordinator-owned per-FVK single-flight guards — see
    // [`ForeignClaimGuards`]. Acquired for the WHOLE body, so concurrent
    // same-key claims serialize instead of racing the durable record.
    claim_guards: &ForeignClaimGuards,
    // Coordinator-owned transient-scan resume checkpoints — see
    // [`super::sync::ForeignScanCheckpointCache`] for the chain-isolation
    // contract.
    scan_checkpoints: &super::sync::ForeignScanCheckpointCache,
    // The claimer wallet's irreversible `shielded_detached` flag, re-read once
    // this claim holds store admission (#4313 review finding 4a2c679745bb).
    // See the re-check below for why the caller's own pre-flight check cannot
    // stand alone.
    detached: &std::sync::atomic::AtomicBool,
    // Claimer's wallet id — keys the durable pending-claim record (under the
    // reserved `ONE_TIME_CLAIM_RECORDS_ACCOUNT` subwallet of this wallet).
    wallet_id: WalletId,
    // Bearer spend authority: carried in a `Zeroizing` buffer so every wallet-layer
    // copy of the one-time spending key is scrubbed on drop (#4204 key-hygiene).
    one_time_sk: zeroize::Zeroizing<[u8; 32]>,
    funding_birth_height: Option<u32>,
    change_address: &OrchardAddress,
    identity_index: u32,
    public_keys: Vec<(IdentityPublicKey, IdentityPublicKeyInCreation)>,
    denomination: u64,
    send_to_address_on_creation_failure: PlatformAddress,
    identity_signer: &IS,
    prover: &P,
) -> Result<OneTimeClaimOutcome, PlatformWalletError>
where
    S: ShieldedStore,
    P: OrchardProver,
    IS: Signer<IdentityPublicKey>,
{
    use grovedb_commitment_tree::{FullViewingKey, Scope, SpendAuthorizingKey, SpendingKey};

    if public_keys.is_empty() {
        return Err(PlatformWalletError::ShieldedBuildError(
            "identity-create-from-one-time-key requires at least one public key".to_string(),
        ));
    }

    // Derive the Orchard key material from the one-time spending key. `from_bytes`
    // returns a `CtOption`; an invalid scalar means the caller handed us a
    // non-key, which is a hard input error.
    //
    // KEY HYGIENE (#4204 finding 1ee08ba70627): orchard 0.14's `SpendingKey`
    // and `SpendAuthorizingKey` are `Copy` types with no `Zeroize` support, so
    // holding them as plain locals would leave complete spend-authority
    // representations in this LONG-LIVED async frame across every network
    // await below. Both are contained in [`super::keys::ScrubOnDrop`] guards
    // (volatile-scrubbed on every exit path) and explicitly dropped at their
    // final use: `sk` right after the derivations here, `ask` right after the
    // bundle build. The `*one_time_sk` deref feeding `from_bytes` is the one
    // unavoidable transient copy (orchard's API takes the array by value); the
    // `Zeroizing` parameter itself scrubs the wallet-layer buffer on drop.
    let sk = super::keys::ScrubOnDrop(
        Option::<SpendingKey>::from(SpendingKey::from_bytes(*one_time_sk)).ok_or_else(|| {
            PlatformWalletError::ShieldedKeyDerivation(
                "one-time spending key is not a valid Orchard SpendingKey".to_string(),
            )
        })?,
    );
    let fvk = FullViewingKey::from(&*sk);
    let ask = super::keys::ScrubOnDrop(SpendAuthorizingKey::from(&*sk));
    let ivk = fvk.to_ivk(Scope::External);
    // The spending key's final use is behind us — scrub it before any network
    // work; only the spend-auth key must survive to the bundle build.
    drop(sk);

    let num_keys = public_keys.len();

    // The invitee's re-derivable MASTER auth key hash: the unique, Platform-indexed
    // handle we recover the created identity by if a claim turns out to have
    // already executed (idempotent-retry recovery — see the spent-nullifier
    // preflight and the broadcast handling below). Captured before `public_keys` is
    // moved into the builder. `None` only if the caller submitted no master auth
    // key (identity creation requires one, so this is defensive).
    let master_key_hash = master_auth_public_key_hash(&public_keys);

    // Snapshot the submitted keys for the defensive empty-`public_keys` fill (the
    // binding signature committed exactly these; same pattern as the pool op).
    let submitted_public_keys: BTreeMap<u32, IdentityPublicKey> = public_keys
        .iter()
        .map(|(key, _)| (key.id(), key.clone()))
        .collect();

    // ---- Per-FVK single-flight (#4313 review finding 979bbc2fcb3c) ----
    //
    // Serialize the COMPLETE claim lifecycle for this invitation key —
    // pending-record lookup, transient scan, transition construction, atomic
    // arming, broadcast, and finalization — before touching any shared state.
    // Without it, two concurrent claims for the same key both see no pending
    // record, build transitions with DIFFERENT padded identity ids, and the
    // second `arm_one_time_claim_record` (INSERT-OR-REPLACE) overwrites the
    // first's byte-exact recovery row while its broadcast may already be on
    // the wire — stranding that identity forever. A parked second caller
    // instead resumes the settled record when the guard lifts. The guard is
    // an async mutex (held across every await below; released on drop, so a
    // cancelled claim cannot wedge the key) owned by the SAME coordinator
    // that owns the record store it protects.
    let claim_record_key = one_time_claim_record_key(&fvk);
    let lifecycle_entry = claim_guards.entry_for(claim_record_key);
    let _lifecycle_guard = lifecycle_entry.lock().await;

    // ---- Store-level lifecycle admission (#4313 review finding cr-7e6c98b9) ----
    //
    // The guard above is per-COORDINATOR: it serializes same-key claims that
    // share this `NetworkShieldedCoordinator`, and nothing else. It cannot
    // order this claim against `clear` / `unregister_wallet` / `remove_wallet`,
    // which take the coordinator's `lifecycle` mutex (a claim takes neither),
    // and it cannot reach a SECOND coordinator or process at all — those get
    // their own `FileBackedShieldedStore` with its own SQLite connections to
    // the same file. Without admission, such a purge deletes this claim's
    // pending record while its transition is broadcasting, and the identity it
    // creates is unrecoverable.
    //
    // So admission is taken where the contention actually is — the store. A
    // destructive operation that already holds admission refuses this claim
    // outright (nothing scanned, built or broadcast); one that starts later
    // sees this lease and waits for it. Both directions are decided by a
    // single atomic store step on each side, so there is no interleaving in
    // which both proceed — see `store::LifecycleAdmission`.
    //
    // Released deterministically after the claim body below. A claim CANCELLED
    // mid-flight (a dropped JNI call) cannot run an async release from `Drop`,
    // so its lease is reclaimed by expiry instead — which errs toward "the
    // purge waits", never toward "the record is deleted".
    let admission = super::store::AdmissionToken::generate()?;
    let admitted = store
        .write()
        .await
        .begin_claim_admission(
            wallet_id,
            admission,
            super::store::admission_now_ms(),
            super::store::CLAIM_LEASE_MS,
        )
        .map_err(|e| {
            PlatformWalletError::Persistence(format!(
                "one-time claim: could not take store lifecycle admission: {e}"
            ))
        })?;
    if !admitted {
        return Err(PlatformWalletError::ShieldedLifecycleBusy {
            reason: "this wallet's shielded state is being cleared or removed; the invitation \
                     claim was not started"
                .to_string(),
        });
    }

    // ---- Removal fence, re-checked UNDER admission (#4313 review finding
    // 4a2c679745bb) ----
    //
    // The caller checks `shielded_detached` before it gets here, and that check
    // cannot stand alone: an FFI caller can resolve and retain the wallet and
    // coordinator, pass the pre-flight check, and only then have
    // `unregister_wallet_with` commit the detach, purge the store, release its
    // destructive barrier, and drop the wallet from the manager. The stale
    // handle would then take a FRESH admission (the barrier is gone), arm a new
    // pending row, and broadcast for a wallet the host has already removed —
    // and its registration tail, finding no manager entry, would leave that row
    // behind.
    //
    // Re-reading the flag HERE is what closes it, because the two admissions
    // are mutually exclusive at the store and totally ordered by it:
    //
    // * If the removal got there first, it has already set `detached` (the flag
    //   is set by `on_admitted`, which runs after IT holds admission), so
    //   either our `begin_claim_admission` above was refused, or the removal
    //   has fully finished and this read sees `true`.
    // * If we got here first, the removal cannot set the flag until it is
    //   admitted, and it cannot be admitted until we release. So a `false` read
    //   under our own lease stays true for the whole claim.
    //
    // Typed as `WalletNotFound`, deliberately NOT `ShieldedLifecycleBusy`: the
    // wallet is gone for good, so this is terminal for this wallet and a retry
    // can only fail again. It is equally not the invitation's terminal
    // `ShieldedInviteAlreadyClaimed` — nothing was spent, and the invitation
    // remains claimable by some other wallet.
    if detached.load(std::sync::atomic::Ordering::Acquire) {
        release_claim_admission(store, admission).await;
        return Err(PlatformWalletError::WalletNotFound(format!(
            "{} was removed from the manager while the invitation claim was starting; nothing \
             was scanned, built or broadcast",
            hex::encode(wallet_id)
        )));
    }

    // ---- Durable per-INVITATION reservation (#4313 review finding cr-9d0e1a44) ----
    //
    // The lease above is per-WALLET: it orders this claim against a purge and
    // nothing else, so it admits BOTH claims of the same invitation. The
    // per-FVK mutex above serializes same-key claims that share THIS
    // coordinator; a second coordinator, or a second process, opens its own
    // SQLite connections to the same file and shares no lock at all. Both then
    // found no pending record, built transitions with DIFFERENT padded identity
    // ids, and the loser's `arm_redrive_under_claim` — an INSERT OR REPLACE —
    // silently overwrote the winner's byte-exact recovery row while the
    // winner's transition was already on the wire.
    //
    // So the invitation's record key is reserved durably, by an insert that
    // cannot overwrite a live row, and the DURABLE row is read back to decide
    // who owns it. The in-process guard stays as the fast path — it parks a
    // same-coordinator second caller before it ever reaches the store — and
    // this is the backstop for the two cases that guard cannot see.
    //
    // The SAME atomic step also returns the durable pending-claim record, if
    // one already exists (#4313 review finding r3767229122). It has to. The
    // record lookup used to run separately, through `pending_redrives`, which
    // in the file-backed store reads an in-memory mirror hydrated once at
    // store OPEN — so a record a PEER store armed after that open was
    // invisible. This claim would then see "no record", build a SECOND
    // transition with a different padded identity id, and replace the peer's
    // only recovery row while the peer's transition was already on the wire,
    // stranding the identity that transition creates. Reading the row inside
    // the reservation's transaction closes the gap by construction: whoever
    // settles the reservation settles what already exists under it, together.
    let claim_records_id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
    let reservation = store
        .write()
        .await
        .reserve_one_time_claim_key(
            claim_records_id,
            claim_record_key,
            admission,
            super::store::admission_now_ms(),
            super::store::CLAIM_LEASE_MS,
        )
        .map_err(|e| {
            // Fail closed. Proceeding without knowing who owns the key is
            // exactly the clobber this reservation exists to prevent.
            PlatformWalletError::Persistence(format!(
                "one-time claim: could not reserve the invitation's claim-record key: {e}"
            ))
        });
    let reservation = match reservation {
        Ok(r) => r,
        Err(e) => {
            release_claim_admission(store, admission).await;
            return Err(e);
        }
    };
    if let super::store::ClaimKeyReservation::Held { holder, expires_at } = reservation.reservation
    {
        debug!(
            holder = %hex::encode(holder.0),
            expires_at,
            resumable_record = reservation.pending.is_some(),
            "one-time claim: another claimant holds this invitation's claim-record key"
        );
    }

    // The heartbeat wraps the COMPLETE admitted claim body — pending-record
    // lookup, resume, transient scan, build, arm, broadcast and confirmation
    // wait alike (#4313 review finding 8de8d05a). See
    // `under_renewed_claim_lease` for why it cannot sit any deeper.
    let claim_result = under_renewed_claim_lease(
        store,
        admission,
        one_time_claim_admitted(
            sdk,
            store,
            scan_checkpoints,
            claim_records_id,
            claim_record_key,
            admission,
            reservation.is_acquired(),
            reservation.pending,
            fvk,
            ivk,
            ask,
            funding_birth_height,
            change_address,
            identity_index,
            public_keys,
            num_keys,
            master_key_hash,
            submitted_public_keys,
            denomination,
            send_to_address_on_creation_failure,
            identity_signer,
            prover,
        ),
    )
    .await;

    release_claim_admission(store, admission).await;
    let (identity_id, identity, recovery_record) = claim_result?;
    Ok(OneTimeClaimOutcome {
        identity_id,
        identity,
        recovery_record,
    })
}

/// Drive `body` to completion while re-stamping the claim lease `admission` on
/// a [`CLAIM_LEASE_RENEW_INTERVAL`](super::store::CLAIM_LEASE_RENEW_INTERVAL)
/// timer, so the protected window is tied to how long the claim actually runs
/// rather than to a fixed guess from whenever the lease was last stamped.
///
/// # Why it wraps the whole admitted body
///
/// The heartbeat originally wrapped only the fresh-build path's broadcast. That
/// left the RESUME path — pending-record lookup, nullifier queries, repeated
/// identity recovery, re-broadcast of the stored transition and an unbounded
/// confirmation wait — running under the INITIAL lease alone, because it
/// returns before the fresh-build path is ever reached (#4313 review finding
/// 8de8d05a). Resume is if anything the slower of the two: it is the path a
/// claim takes precisely because the previous attempt could not resolve
/// quickly.
///
/// A lease that lapses mid-claim is reaped, at which point a concurrent purge
/// counts zero live claims and deletes the very record the in-flight claim
/// needs to recover — so the window has to cover every phase between taking the
/// lease and releasing it. Hoisting it to the single call site around
/// [`one_time_claim_admitted`] covers both paths by construction, and there is
/// no deeper place that could: the two paths only converge here.
///
/// The claim-key reservation taken under the same token rides along, because
/// [`ShieldedStore::renew_claim_admission`] re-stamps it in the same step.
///
/// Cancellation-safe: dropping the returned future drops `body` with it, and
/// the lease is then reclaimed by expiry.
async fn under_renewed_claim_lease<S, F, T>(
    store: &Arc<RwLock<S>>,
    admission: super::store::AdmissionToken,
    body: F,
) -> T
where
    S: ShieldedStore,
    F: std::future::Future<Output = T>,
{
    tokio::pin!(body);
    loop {
        tokio::select! {
            // Bias the body so a renewal tick can never starve the outcome we
            // are actually waiting for.
            biased;
            outcome = &mut body => break outcome,
            _ = tokio::time::sleep(super::store::CLAIM_LEASE_RENEW_INTERVAL) => {
                let renewed = store
                    .write()
                    .await
                    .renew_claim_admission(
                        admission,
                        super::store::admission_now_ms(),
                        super::store::CLAIM_LEASE_MS,
                    );
                match renewed {
                    Ok(true) => {}
                    // A transition may already be on the wire; aborting cannot
                    // un-send it and would only lose the outcome
                    // classification. Carry on, loudly — and note that this is
                    // NOT what protects the claim: the body re-proves ownership
                    // synchronously immediately before every chargeable step
                    // (`assert_claim_lease_before_chargeable_step`), so a claim
                    // that reaches this arm has either not broadcast yet — and
                    // will refuse to — or is already past the point where
                    // stopping helps (#4313 review finding f58ed9d910d8).
                    Ok(false) => error!(
                        "one-time claim lease lapsed or was displaced mid-claim; a \
                         concurrent wallet removal may purge this claim's recovery record"
                    ),
                    Err(e) => warn!(
                        error = %e,
                        "could not renew the one-time claim lease mid-claim"
                    ),
                }
            }
        }
    }
}

/// Re-prove, synchronously and immediately before a CHARGEABLE or otherwise
/// irreversible step, that this claim still owns its purge-protection lease.
///
/// # Why the heartbeat is not enough
///
/// [`under_renewed_claim_lease`] logs a failed renewal and lets the claim run
/// on. Once a transition has been armed that no longer preserves the recovery
/// invariant: [`ShieldedStore::renew_claim_admission`] deliberately refuses to
/// resurrect an expired lease, and destructive admission reaps expired leases
/// before counting live claims. A forward wall-clock adjustment, a run of
/// SQLite errors, or a long executor suspension can therefore leave a live
/// claim non-renewable — and a concurrent `clear` or `unregister_wallet` then
/// counts zero claims and deletes the pending row while the transition is on
/// the wire. For a padded single-note bundle that row is the ONLY handle to
/// the randomized identity id, so losing it strands the identity permanently
/// (#4313 review finding f58ed9d910d8).
///
/// # The gate
///
/// So ownership is proven positively at the last instant before it matters,
/// rather than assumed from a heartbeat that is allowed to fail. A renewal
/// that returns `Ok(true)` both proves the lease is live and re-stamps it (and
/// the claim-key reservation riding the same token), so the broadcast that
/// follows runs at the START of a full lease window rather than at whatever
/// remained of one a long proof build had already spent.
///
/// Everything else fails CLOSED and retryably:
///
/// - `Ok(false)` — the lease is definitively gone (expired or displaced);
/// - `Err(_)` — renewal can no longer PROVE ownership, which for this purpose
///   is the same thing. Treating a store error as "probably still ours" is
///   exactly the repeated-SQLite-error case the finding calls out.
///
/// Refusing here is clean: the caller has not broadcast, so nothing is
/// consumed, nothing is charged, and no proof is burned. The armed record
/// stays on disk with its notes unspent, so the retry resumes it and
/// re-broadcasts the byte-identical transition. If instead the record IS
/// purged in the meantime, that too is harmless — nothing was ever on the
/// wire, so a fresh claim simply rebuilds.
async fn assert_claim_lease_before_chargeable_step<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    admission: super::store::AdmissionToken,
    step: &str,
) -> Result<(), PlatformWalletError> {
    let renewed = store.write().await.renew_claim_admission(
        admission,
        super::store::admission_now_ms(),
        super::store::CLAIM_LEASE_MS,
    );
    match renewed {
        Ok(true) => Ok(()),
        Ok(false) => {
            error!(
                step,
                "one-time claim: the purge-protection lease is gone; refusing to proceed to a \
                 chargeable step without it"
            );
            Err(PlatformWalletError::ShieldedLifecycleBusy {
                reason: format!(
                    "this claim's purge-protection lease lapsed or was displaced before {step}; \
                     without it a concurrent wallet clear or removal could delete the recovery \
                     record while the transition was in flight, so nothing was broadcast — retry \
                     the claim"
                ),
            })
        }
        Err(e) => {
            error!(
                step,
                error = %e,
                "one-time claim: could not prove the purge-protection lease is still held; \
                 refusing to proceed to a chargeable step"
            );
            Err(PlatformWalletError::ShieldedLifecycleBusy {
                reason: format!(
                    "this claim's purge-protection lease could not be verified before {step} \
                     ({e}); proceeding without proof of ownership risks losing the recovery \
                     record mid-flight, so nothing was broadcast — retry the claim"
                ),
            })
        }
    }
}

/// Release a claim's store admission — its lifecycle lease AND the claim-key
/// reservation taken under the same token. Best-effort: both carry an expiry,
/// so a failure here only delays the next claimant of this invitation.
async fn release_claim_admission<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    admission: super::store::AdmissionToken,
) {
    if let Err(e) = store.write().await.end_claim_admission(admission) {
        warn!(
            error = %e,
            "one-time claim: failed to release the store lifecycle admission; it expires on its own"
        );
    }
}

/// The one-time-key claim body, running under a held store admission lease.
///
/// Split out of [`identity_create_from_one_time_key`] purely so the lease is
/// released on EVERY exit — including the many `?` paths — without threading a
/// release through each of them (#4313). The caller owns acquire/release; this
/// function owns the claim.
///
/// `owns_claim_key` is whether this caller won the durable per-invitation
/// reservation. A caller that did NOT is forbidden to build, broadcast or arm
/// anything: it may only RESUME the record the owner left, or refuse. See the
/// branch below.
///
/// `pending_record` is that record, read from DURABLE state in the same atomic
/// step that settled the reservation. It is passed in rather than looked up
/// here on purpose: the file-backed store's `pending_redrives` reads a mirror
/// hydrated at store open, which cannot see a row a peer store armed later, and
/// a claim that trusted it would build a second transition over a live one
/// (#4313 review finding r3767229122).
#[allow(clippy::too_many_arguments)]
async fn one_time_claim_admitted<S, P, IS>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    scan_checkpoints: &super::sync::ForeignScanCheckpointCache,
    claim_records_id: SubwalletId,
    claim_record_key: [u8; 32],
    admission: super::store::AdmissionToken,
    owns_claim_key: bool,
    pending_record: Option<PendingRedrive>,
    fvk: grovedb_commitment_tree::FullViewingKey,
    ivk: grovedb_commitment_tree::IncomingViewingKey,
    ask: super::keys::ScrubOnDrop<grovedb_commitment_tree::SpendAuthorizingKey>,
    funding_birth_height: Option<u32>,
    change_address: &OrchardAddress,
    identity_index: u32,
    public_keys: Vec<(IdentityPublicKey, IdentityPublicKeyInCreation)>,
    num_keys: usize,
    master_key_hash: Option<[u8; 20]>,
    submitted_public_keys: BTreeMap<u32, IdentityPublicKey>,
    denomination: u64,
    send_to_address_on_creation_failure: PlatformAddress,
    identity_signer: &IS,
    prover: &P,
) -> Result<(Identifier, Identity, Option<OneTimeClaimRecoveryRecord>), PlatformWalletError>
where
    S: ShieldedStore,
    P: OrchardProver,
    IS: Signer<IdentityPublicKey>,
{
    // Advisory only: the shielded tree has no height→note-index oracle (a chunk's
    // block_height is the proof-tip height, not per-note inclusion height), so the
    // transient scan cannot seed its start from a height; it bounds itself by
    // value coverage plus a coordinator-owned resume checkpoint (one
    // full-history scan per key per coordinator — see
    // `scan_notes_for_foreign_key`). Logged so
    // the hint is observable and not silently dropped.
    if let Some(h) = funding_birth_height {
        debug!(
            funding_birth_height = h,
            "identity_create_from_one_time_key: birth-height hint (advisory; scan is value-bounded)"
        );
    }

    // ---- Durable pending-claim resume (#4204 review finding c0781f9d387f) ----
    //
    // A claim that broadcast but never confirmed (process death, JNI
    // cancellation, lost result wait) left a persisted record carrying the
    // byte-exact transition and its declared identity id. Consult it BEFORE
    // the transient scan: the record's id survives even for a padded
    // single-note bundle (whose id embeds a random dummy nullifier and is
    // otherwise unrecoverable), so a retry can reconcile or re-drive the
    // byte-identical transition instead of rebuilding one whose preflight
    // would misread the spent notes as a foreign claim
    // (`ShieldedInviteAlreadyClaimed`).
    //
    // The record arrives from the caller, read out of DURABLE state in the same
    // transaction that settled the claim-key reservation — never from the
    // store's startup-hydrated mirror (#4313 review finding r3767229122).
    if let Some(record) = pending_record {
        match resume_one_time_claim(
            sdk,
            store,
            claim_records_id,
            &record,
            admission,
            master_key_hash,
            submitted_public_keys.clone(),
            denomination,
            identity_index,
        )
        .await
        {
            OneTimeClaimResume::Resolved(result) => {
                // Same retention contract as the fresh-build tail below: a
                // successful resume keeps its record until the caller
                // acknowledges durable local registration
                // (#4313 review finding 325ce9fa8f84). This branch had the
                // identical premature-clear ordering.
                let retained = finalize_one_time_claim_record(
                    store,
                    claim_records_id,
                    claim_record_key,
                    &result,
                )
                .await;
                let (identity_id, identity) = result?;
                return Ok((identity_id, identity, retained));
            }
            // The stored transition is unusable (corrupt, or definitively
            // rejected while its notes are provably unspent) — the record has
            // been cleared; build a fresh claim below.
            OneTimeClaimResume::RecordUnusable => {}
        }
    }

    // ---- Losing claimant: RESUME above, or REFUSE here ----
    //
    // Everything past this point builds a fresh transition and arms a fresh
    // record under `claim_record_key`. Only the holder of the durable
    // per-invitation reservation may do that (#4313 review finding
    // cr-9d0e1a44). A claimant that lost the reservation reaches here in
    // exactly two states, and neither may proceed:
    //
    // * the owner has already armed its record — then the resume above ran and
    //   returned, so we are not here at all (or the record was unusable and the
    //   owner will re-arm it, which is still not ours to do);
    // * the owner is admitted but has not armed yet — building here is
    //   precisely the race: two transitions with different padded identity ids,
    //   and whichever arms second overwrites the other's only recovery handle.
    //
    // So refuse, retryably. Nothing was scanned, built or broadcast; the note
    // is untouched; a retry a moment later either finds the owner's record and
    // resumes it, or finds the key free and proceeds as the owner. The storage
    // layer refuses the same case independently — `arm_redrive_under_claim`
    // will not write under a foreign reservation — so this branch is the clean
    // error, not the safety property.
    if !owns_claim_key {
        return Err(PlatformWalletError::ShieldedLifecycleBusy {
            reason: "another claimant currently holds this invitation's claim record; nothing \
                     was scanned, built or broadcast — retry shortly, and the retry will either \
                     resume that claim's outcome or take the invitation over if it lapsed"
                .to_string(),
        });
    }

    // Transient scan: re-derive the one-time key's note(s) from the network.
    let discovered =
        super::sync::scan_notes_for_foreign_key(sdk, scan_checkpoints, &fvk, &ivk, denomination)
            .await?;
    if discovered.is_empty() {
        // No note decrypts under this key — nothing was funded to it (or the
        // wallet hasn't synced far enough to see it yet).
        return Err(PlatformWalletError::ShieldedNoUnspentNotes);
    }

    // Exact-equality selection over the transiently-scanned set: cover exactly
    // `denomination`, gate on `denomination > predicted_fee`. Surfaces
    // `ShieldedInsufficientBalance { available, required }` when the key's notes
    // don't cover the denomination, mirroring the pool-funded neighbor.
    let (selected_refs, total_input, predicted_fee) =
        select_notes_for_denomination(&discovered, denomination, 2, num_keys, sdk.version())?;
    let selected_notes: Vec<ShieldedNote> = selected_refs.into_iter().cloned().collect();

    info!(
        denomination,
        predicted_fee,
        inputs = selected_notes.len(),
        total_input,
        keys = num_keys,
        "IdentityCreateFromOneTimeKey"
    );

    // Idempotent-retry preflight (no persisted record for this key). If this one-time key's
    // selected note(s) are ALREADY spent on chain, a byte-identical claim already
    // executed — so we must NOT rebuild+rebroadcast (that would only earn a
    // `NullifierAlreadySpent` rejection). Everything checked here is re-derived
    // from the invite the invitee holds: the one-time key → its note(s) via the
    // transient scan above, and each note's real nullifier (`ShieldedNote.nullifier`,
    // stamped `note.nullifier(fvk)` during the scan). If spent, recover the
    // previously-created identity by the invitee's own re-derivable MASTER auth key
    // hash (`discover_inner`'s unique-hash probe) and return it as success.
    let selected_nullifiers: Vec<[u8; 32]> = selected_notes.iter().map(|n| n.nullifier).collect();

    // The id that an identity created by THIS claim must carry — the single
    // handle that ties a recovered identity back to this claim's spend, and the
    // reason a MASTER-key-hash hit alone is not evidence of a successful claim
    // (see `recovered_identity_matches_claim`).
    //
    // Consensus derives the new identity id as `double_sha256` over the SORTED
    // set of PUBLISHED action nullifiers (`derive_identity_id_from_actions`) and
    // rejects a transition whose declared id differs, so this is a binding, not a
    // guess.
    //
    // `None` for a single-spend claim: the builder pads to Orchard's 2-action
    // minimum (`num_actions = spends.len().max(2)`) and the padding action's
    // dummy nullifier is randomly generated per build, so it participates in the
    // derivation but cannot be reproduced on a retry. With two or more real
    // spends no padding is added and the published set is exactly
    // `selected_nullifiers`.
    let expected_identity_id =
        (selected_notes.len() >= 2).then(|| identity_id_from_nullifiers(&selected_nullifiers));

    // Idempotent-retry preflight. If this one-time key's selected note(s) are
    // ALREADY spent on chain, this claim can never execute — rebuilding and
    // rebroadcasting would only earn a `NullifierAlreadySpent` rejection and burn
    // a Halo 2 proof. Hand off to the reconciler, which decides between "this
    // claim created that identity" (both bindings verified), "the invitation is
    // gone" (terminal), and "executed but not yet indexed" (retryable).
    // `Unknown` proceeds here — that is safe pre-broadcast: the idempotent
    // broadcast path reconciles via the `NullifierAlreadySpent` verdict, so a
    // transient query failure only costs a harmless rebuild.
    if nullifier_spent_status(sdk, &selected_nullifiers).await == NullifierSpentStatus::Spent {
        // No record was armed on this path — nothing was built or broadcast —
        // so there is none to retain or acknowledge.
        let (identity_id, identity) = recover_executed_one_time_claim(
            sdk,
            master_key_hash,
            expected_identity_id,
            false,
            "the selected note's nullifier is already spent on chain (pre-broadcast preflight)",
        )
        .await?;
        return Ok((identity_id, identity, None));
    }

    // Witness the selected notes against a Platform-recorded anchor from the
    // shared, fully-marked commitment tree (identical probe to the pool op).
    // The anchor's height is not kept on the claim record (int38): a heightless
    // record needs the strongest pruned-anchor evidence before anything releases
    // it, and this record is the only holder of the claim's padded identity id.
    let (spends, anchor, _anchor_height) =
        extract_spends_and_recorded_anchor(sdk, store, &selected_notes).await?;
    let anchor_bytes = anchor.to_bytes();

    let build = build_identity_create_from_shielded_pool_transition(
        public_keys,
        denomination,
        send_to_address_on_creation_failure,
        spends,
        change_address,
        &fvk,
        &ask,
        anchor,
        prover,
        identity_signer,
        [0u8; 36],
        sdk.version(),
    )
    .await
    .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
    // The spend-auth key's final use (the bundle build + spend-auth
    // signatures above) is behind us — scrub it before the broadcast and
    // result wait keep this frame alive across the network.
    drop(ask);

    let identity_id = build.identity_id;

    // Re-assemble the transition from the PoP-signed keys + bundle params
    // (preserving the per-key signatures) and broadcast. The broadcast/wait
    // classification mirrors `identity_create_from_shielded_pool` verbatim, minus
    // the note-reservation bookkeeping (there is no subwallet reservation to
    // release — the spent notes belong to the foreign one-time key).
    let st = sdk
        .identity_create_from_shielded_pool_transition(
            build.public_keys,
            denomination,
            send_to_address_on_creation_failure,
            build.bundle,
        )
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;

    // Persist the pending-claim record BEFORE the broadcast (#4204 review
    // finding c0781f9d387f): once the transition leaves this process, the
    // declared id — the only handle that recovers a padded single-note claim —
    // must already be durable. Fail-closed: nothing has been consumed yet, so
    // refusing to broadcast on a persistence failure is a clean, retryable
    // stop; broadcasting without the record risks an unrecoverable
    // `ShieldedInviteAlreadyClaimed` on the next attempt.
    arm_one_time_claim_record(
        store,
        claim_records_id,
        claim_record_key,
        anchor_bytes,
        &selected_nullifiers,
        &st,
        admission,
        identity_index,
    )
    .await?;

    // The renewal heartbeat that holds the lease open across this broadcast and
    // its confirmation wait runs in the CALLER, around the whole admitted claim
    // body — see `under_renewed_claim_lease`. It used to wrap only this
    // broadcast, which left the resume path (returning above) covered by the
    // initial lease alone (#4313 review finding 8de8d05a).
    //
    // But the heartbeat only LOGS a failed renewal, so it cannot be what
    // authorizes the broadcast. Prove ownership synchronously here instead: the
    // record above is armed and the transition is about to become chargeable
    // and irreversible, and from this point on the record is the only handle
    // that can recover a padded single-note claim's identity. If the lease
    // cannot be re-proven, abort — nothing has been broadcast, the notes are
    // unspent, and the retry resumes this very record
    // (#4313 review finding f58ed9d910d8).
    assert_claim_lease_before_chargeable_step(store, admission, "the claim broadcast").await?;

    let result = broadcast_and_confirm_one_time_claim(
        sdk,
        st,
        identity_id,
        expected_identity_id,
        master_key_hash,
        &selected_nullifiers,
        submitted_public_keys,
        denomination,
    )
    .await;
    // On success the record is RETAINED and handed to the caller, which drops
    // it only once the identity is durably registered locally. Clearing it here
    // — as this used to — lost the padded identity id whenever the process died,
    // the wallet-manager entry was missing, or `persister.store` failed, between
    // this line and the JNI return (#4313 review finding 325ce9fa8f84).
    let retained =
        finalize_one_time_claim_record(store, claim_records_id, claim_record_key, &result).await;
    let (identity_id, identity) = result?;
    Ok((identity_id, identity, retained))
}

/// Broadcast an assembled one-time-key claim transition and drive it to a
/// classified outcome: proven success, idempotent recovery of an
/// already-executed claim, terminal `ShieldedInviteAlreadyClaimed`, definitive
/// `ShieldedBroadcastFailed`, or retryable `ShieldedBroadcastUnconfirmed`.
///
/// Shared by the fresh-build path and the pending-claim resume path
/// (`resume_one_time_claim`), which re-broadcasts the persisted byte-identical
/// transition. `identity_id` is the id the transition DECLARES;
/// `expected_identity_id` is the pre-build re-derivable id (`None` for a
/// padded single-note bundle on the fresh path; always `Some` on the resume
/// path, where the declared id was recovered from the record).
#[allow(clippy::too_many_arguments)]
async fn broadcast_and_confirm_one_time_claim(
    sdk: &Arc<dash_sdk::Sdk>,
    st: StateTransition,
    identity_id: Identifier,
    expected_identity_id: Option<Identifier>,
    master_key_hash: Option<[u8; 20]>,
    claim_nullifiers: &[[u8; 32]],
    submitted_public_keys: BTreeMap<u32, IdentityPublicKey>,
    denomination: u64,
) -> Result<(Identifier, Identity), PlatformWalletError> {
    match st.broadcast(sdk, None).await {
        Ok(()) => {}
        // A `NullifierAlreadySpent` verdict is NOT a failure on this path: it is
        // positive proof a byte-identical claim already executed (the note is
        // consumed on chain). Recover the created identity instead of stranding
        // the retry. Checked before the generic `broadcast_definitely_failed` arm,
        // which would otherwise classify this consensus rejection as a hard failure.
        //
        // POST-BUILD, the reconciler gets `Some(identity_id)` — the id THIS
        // transition committed — never the pre-build `expected_identity_id`
        // (which is deliberately `None` for a padded single-note bundle). The
        // SDK's broadcast internally retries requests, so an accepted first
        // request whose acknowledgement was lost legitimately produces
        // `NullifierAlreadySpent` on the retry; with `None` the reconciler
        // would declare our own successfully created identity permanently
        // lost (`ShieldedInviteAlreadyClaimed`) instead of recovering it by
        // its exact id (#4204 review finding a00cee018e73).
        Err(e) if is_nullifier_already_spent(&e) => {
            return recover_executed_one_time_claim(
                sdk,
                master_key_hash,
                Some(identity_id),
                false,
                &format!("broadcast returned NullifierAlreadySpent: {e}"),
            )
            .await;
        }
        // int38: no refusal arm. v5.1-dev retired `broadcast_definitely_failed`
        // because an endpoint's refusal is unauthenticated and never proves the
        // transition was left out of every mempool; the retained claim record
        // and the result wait below settle it instead.
        Err(e) => {
            warn!(
                derived_id = %identity_id,
                error = %e,
                "IdentityCreateFromOneTimeKey: broadcast returned no verdict; the transition may \
                 have been admitted — falling through to the result wait"
            );
        }
    }

    // Wait for proven execution, mirroring the pool-funded sibling verbatim. A
    // Type-20 IdentityCreateFromShieldedPool proof authenticates the spent
    // nullifiers and resulting identity as an affected-state snapshot; it cannot
    // bind the complete Orchard request, so the current proof contract marks it as
    // affected-state. Use `wait_for_affected_state` — the strict `wait_for_response`
    // would classify every valid claim proof as `ExecutionNotProved`, drop into the
    // ambiguous fallback, and risk reporting a successful claim as unconfirmed.
    let proof_result = match st
        .wait_for_affected_state::<StateTransitionProofResult>(sdk, None)
        .await
    {
        Ok(result) => result,
        // Same idempotent recovery as the broadcast arm: a `NullifierAlreadySpent`
        // verdict surfacing at wait time proves the claim executed, so recover the
        // identity rather than reporting a broadcast failure. Ordered before the
        // generic consensus-rejection arm below (which would classify it as a
        // failure). Same post-build rule as the broadcast arm: pass the id THIS
        // transition committed, never the padding-lossy pre-build one (#4204
        // review finding a00cee018e73).
        Err(wait_err) if is_nullifier_already_spent(&wait_err) => {
            return recover_executed_one_time_claim(
                sdk,
                master_key_hash,
                Some(identity_id),
                false,
                &format!("result wait returned NullifierAlreadySpent: {wait_err}"),
            )
            .await;
        }
        Err(dash_sdk::Error::StateTransitionBroadcastError(e)) if e.cause.is_some() => {
            // A populated cause is a consensus verdict — but for Type 20 a
            // verdict is NOT proof of non-execution: a duplicate unique-key
            // hash makes Drive APPLY the chargeable `UnshieldAction` fallback
            // (the invitation nullifiers are consumed, the fallback address is
            // credited minus the penalty) and record a `PaidConsensusError`,
            // which reaches this arm exactly like a plain rejection. Declaring
            // `ShieldedBroadcastFailed` then would hand the host code 16 —
            // documented as definitive non-execution and safe to retry — for
            // an invitation that is already consumed, and every retry would
            // burn a ~30s proof to earn `NullifierAlreadySpent`. Check the
            // selected nullifiers first: consumed notes prove the transition
            // (or its fallback) APPLIED, so hand off to the reconciler for
            // the terminal claimed/fallback verdict — it distinguishes "this
            // claim created the identity" (recovered as success) from the
            // chargeable fallback / competing claim (terminal
            // `ShieldedInviteAlreadyClaimed`) (#4204 review finding
            // 8d020115b274).
            //
            // The three spent-status outcomes diverge here and only `Unspent`
            // may produce `ShieldedBroadcastFailed`: the host documents that
            // code as definitive non-execution and safe to retry, so it
            // requires PROOF the notes are unconsumed. `Unknown` (query
            // failure / partial response) yields `ShieldedBroadcastUnconfirmed`
            // instead — the armed pending-claim record lets a later retry
            // reconcile with the exact id once the status is queryable.
            match nullifier_spent_status(sdk, claim_nullifiers).await {
                NullifierSpentStatus::Spent => {
                    // `spend_finalized = true`: this claim's own wait returned a
                    // definitive verdict AND the notes are proven consumed, so
                    // "no identity carries our bindings" is the terminal
                    // chargeable-fallback / competing-claim outcome — even when
                    // the colliding unique key was not MASTER and no identity is
                    // findable under either probe.
                    return recover_executed_one_time_claim(
                        sdk,
                        master_key_hash,
                        Some(identity_id),
                        true,
                        &format!(
                            "result wait returned an executed consensus verdict (the invitation \
                             notes are spent — applied claim or chargeable fallback): {e}"
                        ),
                    )
                    .await;
                }
                NullifierSpentStatus::Unspent => {
                    return Err(PlatformWalletError::ShieldedBroadcastFailed(e.to_string()));
                }
                NullifierSpentStatus::Unknown => {
                    return Err(PlatformWalletError::ShieldedBroadcastUnconfirmed {
                        identity_id,
                        reason: format!(
                            "consensus verdict received but the invitation notes' spent status \
                             could not be established; not classifying as a definitive failure \
                             (an applied chargeable fallback would be indistinguishable): {e}"
                        ),
                    });
                }
            }
        }
        Err(wait_err) => {
            warn!(
                derived_id = %identity_id,
                error = %wait_err,
                "IdentityCreateFromOneTimeKey: broadcast accepted but result confirmation failed; \
                 falling back to fetching the identity by its derived id"
            );
            match fetch_identity_with_retries(sdk, identity_id).await {
                Some(mut identity) => {
                    // `identity_id` is the id THIS build derived. Whether finding
                    // an identity under it proves this transition created it
                    // depends on whether the bundle was padded:
                    //
                    // - **Padded (single spend)** — the id embeds a locally
                    //   generated random dummy nullifier that no other party can
                    //   reproduce, so an identity at this id can only have come
                    //   from this transition. The id alone is proof.
                    // - **Not padded (>= 2 spends)** — the id is derived from the
                    //   invitation's real nullifiers alone, so any other holder of
                    //   the same bearer one-time key derives the SAME id under
                    //   their own keys. The on-chain MASTER auth key must be
                    //   checked before this can be called ours.
                    if expected_identity_id.is_some()
                        && !recovered_identity_matches_claim(
                            &identity,
                            expected_identity_id,
                            master_key_hash,
                        )
                    {
                        warn!(
                            derived_id = %identity_id,
                            "IdentityCreateFromOneTimeKey: an identity exists at this claim's \
                             derived id but does not carry the submitted master auth key; another \
                             holder of the same one-time key claimed the invitation first"
                        );
                        return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
                            reason: format!(
                                "identity {identity_id} was created from this invitation's notes \
                                 but does not carry the submitted master authentication key, so it \
                                 belongs to another holder of the one-time key: {wait_err}"
                            ),
                        });
                    }
                    info!(
                        derived_id = %identity_id,
                        "IdentityCreateFromOneTimeKey: result confirmation failed but the identity \
                         was found on chain by its derived id; treating as success"
                    );
                    // Only reached once the identity is proven to be this claim's,
                    // so back-filling the keys this transition itself submitted is
                    // a local-row convenience, not an unproven ownership claim.
                    if identity.public_keys().is_empty() {
                        identity.set_public_keys(submitted_public_keys.clone());
                    }
                    return Ok((identity.id(), identity));
                }
                None => {
                    return Err(PlatformWalletError::ShieldedBroadcastUnconfirmed {
                        identity_id,
                        reason: wait_err.to_string(),
                    });
                }
            }
        }
    };

    let identity = match proof_result {
        StateTransitionProofResult::VerifiedIdentityWithShieldedNullifiers(mut identity, _n) => {
            if identity.id() != identity_id {
                warn!(
                    derived_id = %identity_id,
                    verified_id = %identity.id(),
                    "IdentityCreateFromOneTimeKey: derived id differs from proof-verified id; using \
                     the proof-verified id"
                );
            }
            if identity.public_keys().is_empty() {
                identity.set_public_keys(submitted_public_keys);
            }
            identity
        }
        other => {
            warn!(
                derived_id = %identity_id,
                result = %other,
                "IdentityCreateFromOneTimeKey: unexpected proof-result variant; synthesizing the \
                 identity from the derived id + submitted keys so the local row still lands"
            );
            Identity::new_with_id_and_keys(identity_id, submitted_public_keys, sdk.version())
                .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?
        }
    };

    info!(
        denomination,
        identity_id = %identity.id(),
        "IdentityCreateFromOneTimeKey broadcast succeeded"
    );
    Ok((identity.id(), identity))
}

/// The synthetic ZIP-32 account index that keys durable one-time-claim records
/// in the [`ShieldedStore`].
///
/// Claim records reuse the store's persisted [`PendingRedrive`] rows (byte-exact
/// transition + nullifiers + anchor), but live under this reserved subwallet so
/// the spend-redrive sync pass — which iterates REAL Orchard accounts — never
/// re-broadcasts or prunes them; their lifecycle is owned entirely by
/// [`identity_create_from_one_time_key`]. ZIP-32 account indices are hardened
/// (`< 2^31`), so `u32::MAX` cannot collide with a real subwallet.
pub(super) const ONE_TIME_CLAIM_RECORDS_ACCOUNT: u32 = u32::MAX;

/// Deterministic record key for a one-time claim: every retry of the same
/// invitation re-derives the same key from the one-time FVK, which is exactly
/// what lets a retry find the record a crashed attempt left behind. Domain-
/// separated so it can never collide with an activity-entry id (sha256 of
/// visible output cmxs) sharing the `PendingRedrive.activity_id` keyspace.
fn one_time_claim_record_key(fvk: &grovedb_commitment_tree::FullViewingKey) -> [u8; 32] {
    use dashcore::hashes::{sha256, Hash};

    let mut preimage = Vec::with_capacity(96 + 33);
    preimage.extend_from_slice(b"platform-wallet:one-time-claim:v1");
    preimage.extend_from_slice(&fvk.to_bytes());
    sha256::Hash::hash(&preimage).to_byte_array()
}

/// The shared per-FVK lifecycle mutex handed to every same-key claimer.
type ClaimGuard = Arc<tokio::sync::Mutex<()>>;

/// One registry row: the guard key (see `one_time_claim_record_key`) paired
/// with a non-owning handle to its guard, so abandoned keys are pruned on the
/// next acquisition rather than pinning the mutex alive.
type ClaimGuardEntry = ([u8; 32], std::sync::Weak<tokio::sync::Mutex<()>>);

/// Per-FVK single-flight guards for the one-time-key claim lifecycle.
///
/// Owned by `NetworkShieldedCoordinator` (the same owner as the durable
/// pending-claim record store the guard protects). Two concurrent
/// [`identity_create_from_one_time_key`] calls for the SAME foreign key would
/// otherwise both observe no pending record, build two transitions whose
/// padded single-note identity ids differ (random padding nullifier), and
/// race `arm_one_time_claim_record` — whose store implementation is an
/// INSERT-OR-REPLACE — so the loser's byte-exact recovery row is silently
/// overwritten and its identity becomes unrecoverable; either caller could
/// also finalize (clear) the shared row while the other is mid-broadcast
/// (#4313 review finding 979bbc2fcb3c / cr-4808dde4). The guard therefore
/// spans the COMPLETE lifecycle — pending-record lookup, transient scan,
/// transition construction, atomic arming, broadcast, and finalization — not
/// just the scan-checkpoint window: the second caller parks until the first
/// settles, then resumes that outcome through the persisted record instead of
/// double-spending the invitation.
///
/// Mechanics: `entry_for` hands every same-key caller the SAME
/// `Arc<tokio::sync::Mutex<()>>` (a live entry is always upgraded, never
/// replaced), whose async lock is cancellation-safe — dropping a parked or
/// mid-claim future releases it. The map holds only `Weak` handles, pruned on
/// every acquisition, so abandoned keys cost nothing and hostile key churn
/// cannot grow the map beyond the keys currently in flight.
#[derive(Default)]
pub struct ForeignClaimGuards {
    entries: std::sync::Mutex<Vec<ClaimGuardEntry>>,
}

impl ForeignClaimGuards {
    /// The shared lifecycle mutex for `key`. Callers `.lock().await` the
    /// returned handle and hold the guard across the whole claim; the
    /// internal registry lock is sync-only and released before any await.
    fn entry_for(&self, key: [u8; 32]) -> ClaimGuard {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|(_, weak)| weak.strong_count() > 0);
        if let Some((_, weak)) = entries.iter().find(|(k, _)| *k == key) {
            if let Some(existing) = weak.upgrade() {
                return existing;
            }
        }
        let fresh = Arc::new(tokio::sync::Mutex::new(()));
        entries.retain(|(k, _)| *k != key);
        entries.push((key, Arc::downgrade(&fresh)));
        fresh
    }
}

/// Look up the pending-claim record for `key` through
/// [`ShieldedStore::pending_redrives`].
///
/// **Test-only.** The production claim path no longer reads the record this
/// way: `pending_redrives` is served from the file-backed store's
/// startup-hydrated mirror, which cannot see a row a peer store armed after
/// our open, and a claim that trusted it would build a second transition over
/// a live one (#4313 review finding r3767229122). The real lookup now comes
/// back from [`ShieldedStore::reserve_one_time_claim_key`], read out of
/// durable state in the same transaction that settles the reservation.
///
/// It survives here because the arm/clear/finalize round-trip tests drive the
/// [`InMemoryShieldedStore`], whose map IS its durable state — so for THEM the
/// two reads are the same read.
///
/// [`InMemoryShieldedStore`]: super::store::InMemoryShieldedStore
#[cfg(test)]
async fn find_one_time_claim_record<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    key: [u8; 32],
) -> Result<Option<PendingRedrive>, PlatformWalletError> {
    store
        .read()
        .await
        .pending_redrives(id)
        .map(|records| records.into_iter().find(|r| r.activity_id == key))
        .map_err(|e| {
            PlatformWalletError::Persistence(format!(
                "pending one-time-claim record lookup failed; refusing to build a fresh claim \
                 while an earlier attempt's record may exist: {e}"
            ))
        })
}

/// Persist the pending-claim record UNDER this claim's store-level admission
/// lease. Called BEFORE the broadcast; a failure aborts the claim (fail-closed
/// — see the call site).
///
/// The lease re-check and the record write are one atomic store step
/// ([`ShieldedStore::arm_redrive_under_claim`]), which is what leaves no gap
/// between "still admitted" and "record written" for a concurrent
/// `clear`/`unregister_wallet` to slot into (#4313). The same step re-stamps
/// the lease (and the claim-key reservation riding the same token), so the
/// record is written into a freshly extended window rather than into whatever
/// remained of one a long scan had already spent. Keeping that window open past
/// this point is the heartbeat's job — see [`under_renewed_claim_lease`].
///
/// A lost lease is a hard stop, not a warning: nothing has been broadcast yet,
/// so refusing is clean and retryable, whereas broadcasting without the record
/// is how a padded single-note claim's identity becomes unrecoverable.
///
/// `identity_index` is persisted with the record because it is the ONE part of
/// the claim's binding the transition cannot witness — a purely local DIP-9
/// slot that appears nowhere in `st_bytes` (#4313 review finding 5d4d6efa).
/// Everything else a resume checks is re-derived from the stored bytes; see
/// [`resume_one_time_claim`].
#[allow(clippy::too_many_arguments)]
async fn arm_one_time_claim_record<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    key: [u8; 32],
    anchor: [u8; 32],
    nullifiers: &[[u8; 32]],
    st: &StateTransition,
    admission: super::store::AdmissionToken,
    identity_index: u32,
) -> Result<(), PlatformWalletError> {
    use dpp::serialization::PlatformSerializable;

    let st_bytes = st
        .serialize_to_bytes()
        .map_err(|e| PlatformWalletError::ShieldedBuildError(e.to_string()))?;
    let admitted = store
        .write()
        .await
        .arm_redrive_under_claim(
            id,
            PendingRedrive {
                activity_id: key,
                anchor,
                nullifiers: nullifiers.to_vec(),
                st_bytes,
                attempts: 0,
                identity_index: Some(identity_index),
                identity_nonce_finalized: false,
                identity_user_abandoned: false,
                anchor_height: None,
            },
            admission,
            super::store::admission_now_ms(),
            super::store::CLAIM_LEASE_MS,
        )
        .map_err(|e| {
            PlatformWalletError::Persistence(format!(
                "failed to persist the pending one-time-claim record before broadcast: {e}"
            ))
        })?;
    if !admitted {
        return Err(PlatformWalletError::ShieldedLifecycleBusy {
            reason: "this claim's store admission lapsed before its recovery record could be \
                     written (the wallet was cleared or removed, or the claim outran its lease); \
                     nothing was broadcast — retry the claim"
                .to_string(),
        });
    }
    Ok(())
}

/// Drop the pending-claim record. Best-effort: a failure only means the next
/// attempt resumes a settled record, which re-resolves to the same outcome.
async fn clear_one_time_claim_record<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    key: [u8; 32],
) {
    if let Err(e) = store.write().await.clear_redrive(id, &key) {
        warn!(
            error = %e,
            "one-time claim: failed to clear the pending-claim record"
        );
    }
}

/// Settle the pending-claim record for a claim that has reached an outcome.
///
/// # A success does NOT clear the record
///
/// It used to. That dropped the row the moment the transition resolved, which
/// is BEFORE control returns through `PlatformWallet::identity_create_from_one_time_key`,
/// where the identity is added to the local manager and handed to the host
/// persister. A process death in that gap — or a missing wallet-manager entry,
/// or a `persister.store` failure — lost the exact padded identity ID before
/// the JNI caller ever received it. The retry then re-scans, finds the notes
/// spent, and `recover_executed_one_time_claim` has no `expected_identity_id`
/// to bind against (a single-spend bundle's id embeds a randomly generated
/// dummy nullifier), so it returns the TERMINAL
/// [`PlatformWalletError::ShieldedInviteAlreadyClaimed`] and the identity is
/// stranded forever (#4313 review finding 325ce9fa8f84).
///
/// So the record — the only durable copy of that id — is RETAINED across the
/// native return and the host persistence handoff, and is dropped only by
/// [`acknowledge_one_time_claim_registration`], once local registration has
/// been durably acknowledged. A retry that arrives before that acknowledgement
/// finds the record, sees the notes already spent, and recovers the identity by
/// its declared id instead of failing terminally — which is exactly the
/// reconciliation state the finding asks for, expressed in the machinery that
/// already exists rather than a second one beside it.
///
/// Retaining is cheap and self-healing: the record is one row keyed by the
/// invitation, the resume path is idempotent, and the acknowledgement clears it
/// on the very next successful pass.
///
/// # What IS cleared here
///
/// Only the terminal [`PlatformWalletError::ShieldedInviteAlreadyClaimed`]: it
/// means the invitation produced no identity this wallet can claim, so no
/// future retry can get anything from the record. Every other error keeps it —
/// `ShieldedBroadcastUnconfirmed` (and unproven failures) are exactly the
/// outcomes whose retry must find the declared id again.
///
/// Returns the retained record's coordinates when the caller is expected to
/// acknowledge it, i.e. on success.
async fn finalize_one_time_claim_record<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    key: [u8; 32],
    result: &Result<(Identifier, Identity), PlatformWalletError>,
) -> Option<OneTimeClaimRecoveryRecord> {
    match result {
        Ok(_) => Some(OneTimeClaimRecoveryRecord {
            claim_records_id: id,
            claim_record_key: key,
        }),
        Err(PlatformWalletError::ShieldedInviteAlreadyClaimed { .. }) => {
            clear_one_time_claim_record(store, id, key).await;
            None
        }
        Err(_) => None,
    }
}

/// The retained recovery record of a SUCCESSFUL one-time-key claim: the durable
/// row that still holds the claim's transition, and with it the exact identity
/// id a padded single-note bundle cannot otherwise reproduce.
///
/// Handed back to the caller by [`identity_create_from_one_time_key`] so the
/// row outlives the native return and the host persistence handoff. The caller
/// registers the identity locally and, ONLY once that registration is durably
/// acknowledged, passes this to [`acknowledge_one_time_claim_registration`]
/// (#4313 review finding 325ce9fa8f84).
///
/// Dropping this value without acknowledging is safe and is the intended
/// behaviour on a registration failure: the record simply survives, and the
/// next claim of the same invitation resumes it and recovers the identity.
#[derive(Debug, Clone, Copy)]
pub struct OneTimeClaimRecoveryRecord {
    claim_records_id: SubwalletId,
    claim_record_key: [u8; 32],
}

/// A successful one-time-key claim: the created identity, plus the recovery
/// record that must be acknowledged once the identity is durably registered
/// locally. See [`OneTimeClaimRecoveryRecord`].
#[derive(Debug)]
pub struct OneTimeClaimOutcome {
    /// The new identity's id.
    pub identity_id: Identifier,
    /// The proof-verified identity, for local registration.
    pub identity: Identity,
    /// The retained recovery record, or `None` when this claim armed no record
    /// (the pre-broadcast preflight recovered an already-executed claim without
    /// ever building one).
    pub recovery_record: Option<OneTimeClaimRecoveryRecord>,
}

/// Drop a successful claim's retained recovery record, now that its identity is
/// durably registered in the caller's local state.
///
/// This is the acknowledgement half of the retention introduced for
/// #4313 review finding 325ce9fa8f84: until it runs, a retry of the same
/// invitation resumes the record and recovers the identity by its declared id
/// rather than failing terminally.
///
/// Call it ONLY after local registration is durable — after the identity is in
/// the manager AND its changeset reached the host persister. Calling it early
/// re-opens the exact window the retention closes; not calling it at all costs
/// one stale row that the next claim of this invitation resolves.
///
/// Best-effort by construction: a failure to clear leaves a settled record that
/// the next attempt re-resolves to the same identity.
pub async fn acknowledge_one_time_claim_registration<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    record: OneTimeClaimRecoveryRecord,
) {
    debug!(
        claim_record_key = %hex::encode(record.claim_record_key),
        "one-time claim: identity durably registered; releasing the retained recovery record"
    );
    clear_one_time_claim_record(store, record.claim_records_id, record.claim_record_key).await;
}

/// Describe the first way a resume attempt's arguments disagree with the
/// transition the earlier attempt submitted, or `None` when they agree.
///
/// Every field compared here is one the resume would otherwise act on with the
/// caller's value while broadcasting the *stored* bytes — the exact mis-binding
/// #4313 review finding 195efdd4ae21 describes. The comparison is on the
/// derived-from-transition side, so it is a statement about what is on the wire,
/// not about what some parallel record claims.
///
/// Key comparison is exact and whole-set: `IdentityPublicKey` compares by id,
/// purpose, security level, key type, read-only flag, contract bounds and key
/// data, so a retry that keeps the ids but swaps the key material — the case
/// that would register a foreign identity at this wallet's slot — is caught.
/// A retry that merely *reorders* the same keys is not a mismatch: both sides
/// are `BTreeMap`s keyed by key id.
#[allow(clippy::too_many_arguments)]
fn one_time_claim_binding_mismatch(
    stored_public_keys: &BTreeMap<u32, IdentityPublicKey>,
    stored_master_key_hash: Option<[u8; 20]>,
    stored_denomination: u64,
    stored_identity_index: Option<u32>,
    submitted_public_keys: &BTreeMap<u32, IdentityPublicKey>,
    submitted_master_key_hash: Option<[u8; 20]>,
    submitted_denomination: u64,
    submitted_identity_index: u32,
) -> Option<String> {
    // The one field compared against the RECORD rather than against the
    // transition, because the transition cannot witness it: the DIP-9 slot is a
    // purely local placement (#4313 review finding 5d4d6efa). Checked first —
    // it is the cheapest comparison, and a slot mismatch is the case whose
    // consequence is least visible: `IdentityManager::add_identity` rejects a
    // duplicate identity id but inserts into an OCCUPIED slot without
    // complaint, so a retry that presents the original keys at a different
    // index would silently displace whatever the wallet tracked there.
    //
    // `None` means the record predates the column; there is nothing to compare,
    // and that record keeps exactly the transitive binding it was written
    // under.
    if let Some(stored) = stored_identity_index {
        if stored != submitted_identity_index {
            return Some(format!(
                "identity index: the earlier attempt was registering at local slot {stored}, \
                 retry asked for slot {submitted_identity_index}"
            ));
        }
    }
    if stored_denomination != submitted_denomination {
        return Some(format!(
            "denomination: stored transition spends {stored_denomination}, retry asked for \
             {submitted_denomination}"
        ));
    }
    if stored_master_key_hash != submitted_master_key_hash {
        // The MASTER auth key hash is the handle `recover_executed_one_time_claim`
        // probes Platform with. Recovering under a hash that is not in the stored
        // transition can only find someone else's identity.
        return Some(format!(
            "master authentication key hash: stored transition carries {}, retry presented {}",
            stored_master_key_hash.map_or_else(|| "none".to_string(), hex::encode),
            submitted_master_key_hash.map_or_else(|| "none".to_string(), hex::encode),
        ));
    }
    if stored_public_keys != submitted_public_keys {
        return Some(format!(
            "public key set: stored transition carries {} key(s) (ids {:?}), retry presented {} \
             key(s) (ids {:?})",
            stored_public_keys.len(),
            stored_public_keys.keys().collect::<Vec<_>>(),
            submitted_public_keys.len(),
            submitted_public_keys.keys().collect::<Vec<_>>(),
        ));
    }
    None
}

/// Outcome of attempting to resume a persisted pending claim.
enum OneTimeClaimResume {
    /// The record drove the claim to an outcome — return it to the caller.
    Resolved(Result<(Identifier, Identity), PlatformWalletError>),
    /// The record cannot drive an outcome (corrupt, wrong transition type, or
    /// definitively rejected with its notes proven unspent). It has been
    /// cleared; the caller builds a fresh claim.
    RecordUnusable,
}

/// Resume a claim from its persisted record (#4204 review finding
/// c0781f9d387f): recover by the DECLARED id when the notes are already
/// consumed, otherwise re-broadcast the byte-identical stored transition —
/// never rebuild while the record is live, because a rebuilt padded bundle
/// derives a fresh random id and orphans the recorded one.
///
/// # The resumed claim is bound to the STORED transition, not to this call
///
/// (#4313 review finding 195efdd4ae21.) The record is found by wallet id and
/// one-time FVK alone, so nothing about the lookup says *which* identity the
/// original attempt was creating. This call's `master_key_hash`,
/// `submitted_public_keys` and `denomination` are therefore treated as
/// **assertions to check**, never as inputs to act on: every one of them is
/// re-derived from `record.st_bytes` — the byte-exact transition the earlier
/// attempt actually put (or is about to put) on the wire — and the derived
/// values are what drive recovery, the empty-proof-result backfill and the
/// re-broadcast.
///
/// Deriving rather than persisting the binding is deliberate wherever the
/// transition can witness it (`public_keys` are exactly what the binding
/// signature committed to; `denomination` is the value that leaves the pool):
/// a derived binding cannot drift from what was submitted the way a separately
/// persisted copy could.
///
/// If the caller's arguments disagree with the transition, this is **not** a
/// resume of the same claim — it is a request to create a different identity
/// from an invitation already committed elsewhere — and it fails closed with
/// [`PlatformWalletError::ShieldedClaimBindingMismatch`]: nothing is
/// re-broadcast (so no chargeable resubmission and no burned proof), and the
/// record is left intact for a retry that presents the original arguments.
///
/// ## What this binds
///
/// Derived from `record.st_bytes`: the submitted key set (by id and content),
/// the MASTER authentication key hash used for idempotent recovery, and the
/// denomination.
///
/// Read from the record itself: `identity_index`, the local DIP-9 slot the
/// returned identity is registered at (#4313 review finding 5d4d6efa). It is
/// the one part of the binding the transition CANNOT witness — a purely local
/// placement that appears nowhere in `st_bytes` — so it is persisted with the
/// claim precisely because there is nothing to derive it from.
///
/// It used to be left to a *transitive* argument: the identity's keys are
/// derived from the wallet seed at that slot, so a retry naming a different
/// slot ought to present different keys and be refused by the key check. That
/// argument leaves the caller free to pair slot `i` with keys derived at slot
/// `j`, and the consequence is not symmetric with a first attempt's: a retry
/// that presents the ORIGINAL keys with a different index reaches
/// `IdentityManager::add_identity`, which rejects a duplicate identity id but
/// inserts into an occupied slot without complaint — silently displacing
/// whatever identity the wallet already tracked there. Persisting the slot and
/// comparing it closes that directly.
///
/// A record written before the column existed carries `None`; there is nothing
/// to compare it against, so the check is skipped and that record keeps exactly
/// the transitive binding it was written under.
#[allow(clippy::too_many_arguments)]
async fn resume_one_time_claim<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    claim_records_id: SubwalletId,
    record: &PendingRedrive,
    admission: super::store::AdmissionToken,
    master_key_hash: Option<[u8; 20]>,
    submitted_public_keys: BTreeMap<u32, IdentityPublicKey>,
    denomination: u64,
    identity_index: u32,
) -> OneTimeClaimResume {
    use dpp::serialization::PlatformDeserializableTrusted;
    use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::accessors::IdentityCreateFromShieldedPoolTransitionAccessorsV0;

    let st = match StateTransition::deserialize_from_bytes_trusted(&record.st_bytes) {
        Ok(st) => st,
        Err(e) => {
            warn!(
                error = %e,
                "one-time claim resume: stored transition failed to deserialize; dropping the \
                 record and rebuilding"
            );
            clear_one_time_claim_record(store, claim_records_id, record.activity_id).await;
            return OneTimeClaimResume::RecordUnusable;
        }
    };
    // Everything the resume acts on comes from HERE — the stored transition —
    // not from this call's arguments. See the fn docs.
    let (declared_id, stored_public_keys, stored_denomination) = match &st {
        StateTransition::IdentityCreateFromShieldedPool(t) => {
            let keys: BTreeMap<u32, IdentityPublicKey> = t
                .public_keys()
                .iter()
                .map(|key_in_creation| {
                    let key: IdentityPublicKey = key_in_creation.into();
                    (key.id(), key)
                })
                .collect();
            (t.identity_id(), keys, t.denomination())
        }
        other => {
            warn!(
                transition = %other.name(),
                "one-time claim resume: stored record does not carry a shielded identity-create \
                 transition; dropping the record and rebuilding"
            );
            clear_one_time_claim_record(store, claim_records_id, record.activity_id).await;
            return OneTimeClaimResume::RecordUnusable;
        }
    };
    let stored_master_key_hash = master_auth_public_key_hash_of(stored_public_keys.values());

    // Fail closed on any disagreement between what the caller asked for and
    // what the earlier attempt committed. Checked BEFORE the spent-nullifier
    // probe and the re-broadcast, so a mismatched retry costs nothing and
    // changes nothing — in particular the record survives for a correct retry.
    if let Some(mismatch) = one_time_claim_binding_mismatch(
        &stored_public_keys,
        stored_master_key_hash,
        stored_denomination,
        record.identity_index,
        &submitted_public_keys,
        master_key_hash,
        denomination,
        identity_index,
    ) {
        warn!(
            declared_id = %declared_id,
            mismatch,
            "one-time claim resume: retry arguments do not match the stored transition; refusing \
             to resume rather than mis-binding the original identity"
        );
        return OneTimeClaimResume::Resolved(Err(
            PlatformWalletError::ShieldedClaimBindingMismatch { mismatch },
        ));
    }

    // Past the gate the two agree, so the derived values are used from here on
    // — the transition is the source of truth by construction, and reading them
    // from it keeps that true even if the check above is ever relaxed.
    let master_key_hash = stored_master_key_hash;
    let submitted_public_keys = stored_public_keys;
    let denomination = stored_denomination;

    info!(
        declared_id = %declared_id,
        nullifiers = record.nullifiers.len(),
        "one-time claim: resuming from the persisted pending-claim record"
    );

    let status = nullifier_spent_status(sdk, &record.nullifiers).await;
    if status == NullifierSpentStatus::Spent {
        // The recorded claim (or a competitor) already consumed the notes.
        // The DECLARED id — unrecoverable without the record for a padded
        // bundle — lets the reconciler bind a created identity to this claim.
        return OneTimeClaimResume::Resolved(
            recover_executed_one_time_claim(
                sdk,
                master_key_hash,
                Some(declared_id),
                false,
                "resume: the recorded pending claim's notes are already spent on chain",
            )
            .await,
        );
    }

    // Unspent or Unknown: re-drive the byte-identical transition through the
    // same broadcast/confirm classification as a fresh claim. Byte-identical
    // re-broadcast is fund-safe (identical nullifiers cannot double-spend) and
    // preserves the recorded id.
    //
    // Same chargeable-step gate as the fresh-build path: a re-broadcast is a
    // resubmission that can execute and be charged for, so it may not proceed
    // on a lease the heartbeat can no longer prove
    // (#4313 review finding f58ed9d910d8). Aborting here is clean — the stored
    // record and its unspent notes are untouched, and the next retry resumes
    // exactly this record again.
    if let Err(e) =
        assert_claim_lease_before_chargeable_step(store, admission, "the claim re-broadcast").await
    {
        return OneTimeClaimResume::Resolved(Err(e));
    }

    let result = broadcast_and_confirm_one_time_claim(
        sdk,
        st,
        declared_id,
        Some(declared_id),
        master_key_hash,
        &record.nullifiers,
        submitted_public_keys,
        denomination,
    )
    .await;

    if status == NullifierSpentStatus::Unspent {
        if let Err(PlatformWalletError::ShieldedBroadcastFailed(reason)) = &result {
            // Definitive rejection of the STORED transition while its notes
            // are proven unconsumed (e.g. its anchor aged out of Platform's
            // recorded set): this record can never land. Clear it and build a
            // fresh claim in this same call.
            warn!(
                declared_id = %declared_id,
                reason,
                "one-time claim resume: stored transition is definitively rejected and its notes \
                 are unspent; dropping the record and rebuilding"
            );
            clear_one_time_claim_record(store, claim_records_id, record.activity_id).await;
            return OneTimeClaimResume::RecordUnusable;
        }
    }

    OneTimeClaimResume::Resolved(result)
}

// -------------------------------------------------------------------------
// Internal helpers (free fns)
// -------------------------------------------------------------------------

/// Convert `keys`'s default `PaymentAddress` to an `OrchardAddress`.
fn default_orchard_address(
    keys: &AccountViewingKeys,
) -> Result<OrchardAddress, PlatformWalletError> {
    payment_address_to_orchard(&keys.default_address)
}

/// Checkpoint depths probed for a Platform-recorded anchor. Kept equal to the
/// commitment tree's `max_checkpoints` retention (the store is opened with
/// `100` — see `PlatformWalletManager`) so the probe reaches every checkpoint
/// the tree still holds and no further: deeper checkpoints are pruned, so
/// probing past this bound is wasted work. Coupled by convention — if that
/// retention changes, update this in lockstep.
const MAX_ANCHOR_PROBE_DEPTH: usize = 100;

/// Extract `SpendableNote` structs with Merkle witnesses and an anchor
/// Platform has recorded.
///
/// A shielded spend's proof is accepted only if its anchor is a
/// commitment-tree root Platform recorded (`validate_anchor_exists`).
/// Platform records one anchor per block, but an index-chunk sync routinely
/// leaves the wallet's tree mid-block, so the depth-0 (current) root is
/// frequently a value Platform never recorded — building against it
/// unconditionally is what made such spends fail and never land.
///
/// This fetches Platform's recorded anchor set (outside the store lock), then
/// selects the shallowest checkpoint depth whose root is in that set — depth 0
/// being the fully-synced fast path — witnessing every note at that same depth
/// so the anchor and the authentication paths agree (the builder derives the
/// anchor from the witnesses via `MerklePath::root`, so a per-note disagreement
/// would surface downstream as `AnchorMismatch`). When no probed depth has a
/// recorded root it returns the retryable
/// [`PlatformWalletError::ShieldedNoRecordedAnchor`] rather than broadcasting a
/// spend Platform is guaranteed to reject.
///
/// Returns the proved height of the recorded anchor set the anchor was chosen
/// from too: the anchor was in the set at that height, so a set proved at or
/// above it that lacks the anchor shows it pruned rather than newer.
async fn extract_spends_and_recorded_anchor<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    notes: &[ShieldedNote],
) -> Result<(Vec<SpendableNote>, Anchor, u64), PlatformWalletError> {
    // Nothing selected — fail before the network round-trip.
    if notes.is_empty() {
        return Err(PlatformWalletError::ShieldedBuildError(
            "no spendable notes selected — anchor undefined".to_string(),
        ));
    }

    // Fetch the recorded anchor set OUTSIDE the store lock so the network
    // round-trip doesn't serialize with other store users, and so the lock is
    // held only for the mutually-consistent depth/witness probe below.
    let (dash_sdk::query_types::ShieldedAnchors(recorded_anchors), metadata) =
        dash_sdk::query_types::ShieldedAnchors::fetch_current_with_metadata(sdk).await?;
    let recorded: HashSet<[u8; 32]> = recorded_anchors.into_iter().collect();

    // Hold a single read lock across the whole probe so the checkpoint depths
    // and the per-note witnesses stay mutually consistent: a concurrent sync
    // checkpointing mid-probe would otherwise shift the depth indices out from
    // under us.
    let store = store.read().await;
    select_recorded_spends(&*store, notes, &recorded)
        .map(|(spends, anchor)| (spends, anchor, metadata.height))
}

/// Pick the shallowest checkpoint depth whose tree root is in `recorded`,
/// witnessing every note at that depth. Pure (no SDK, no async), so the depth
/// walk can be unit-tested against a real commitment tree.
///
/// Depth 0 is the current tree state (the fully-synced fast path). Deeper
/// checkpoints are older and hold strictly fewer positions, so the probe stops
/// as soon as a selected note is no longer witnessable at a depth — no deeper
/// checkpoint could contain it. Returns
/// [`PlatformWalletError::ShieldedNoRecordedAnchor`] when no probed depth has a
/// recorded root (a clean, retryable outcome — nothing is broadcast).
fn select_recorded_spends<S: ShieldedStore>(
    store: &S,
    notes: &[ShieldedNote],
    recorded: &HashSet<[u8; 32]>,
) -> Result<(Vec<SpendableNote>, Anchor), PlatformWalletError> {
    use grovedb_commitment_tree::ExtractedNoteCommitment;

    // Deserialize each note and decode its commitment ONCE — both are
    // independent of the checkpoint depth, so hoisting them out of the probe
    // keeps the depth walk cheap (each probed depth only re-witnesses).
    let prepared: Vec<(u64, grovedb_commitment_tree::Note, ExtractedNoteCommitment)> = notes
        .iter()
        .map(|note| {
            let orchard_note = deserialize_note(&note.note_data).ok_or_else(|| {
                PlatformWalletError::ShieldedBuildError(format!(
                    "Failed to deserialize note at position {}",
                    note.position
                ))
            })?;
            let cmx = ExtractedNoteCommitment::from_bytes(&note.cmx)
                .into_option()
                .ok_or_else(|| {
                    PlatformWalletError::ShieldedBuildError(format!(
                        "invalid stored cmx for note at position {}",
                        note.position
                    ))
                })?;
            Ok((note.position, orchard_note, cmx))
        })
        .collect::<Result<_, PlatformWalletError>>()?;

    // Build every selected note's `SpendableNote` plus the shared anchor at a
    // single checkpoint `depth`.
    //
    // `strict` (depth 0 only): a missing/failed witness is a hard
    // `ShieldedMerkleWitnessUnavailable` (the note is expected to be witnessable
    // at the current tip). At depth > 0, `Ok(None)` means the note post-dates
    // this older checkpoint, so the depth is unusable — return `Ok(None)` and
    // let the caller stop probing deeper; a genuine store `Err` (poisoned mutex,
    // IO, tree corruption) is logged — the probe would otherwise discard the
    // message — and likewise treated as an unusable depth rather than aborting,
    // so a transient read can't strand a spend a shallower depth already
    // covered. An anchor disagreement across notes is always a hard error (the
    // spend builder would reject it downstream).
    let build_at_depth = |depth: usize,
                          strict: bool|
     -> Result<Option<(Vec<SpendableNote>, Anchor)>, PlatformWalletError> {
        let mut spends = Vec::with_capacity(prepared.len());
        let mut anchor: Option<Anchor> = None;
        for (position, note, cmx) in &prepared {
            let merkle_path = match store.witness_at_depth(*position, depth) {
                Ok(Some(path)) => path,
                Ok(None) if strict => {
                    return Err(PlatformWalletError::ShieldedMerkleWitnessUnavailable(format!(
                        "no witness available for note at position {position} (not marked, or pruned past this position)"
                    )));
                }
                Err(e) if strict => {
                    return Err(PlatformWalletError::ShieldedMerkleWitnessUnavailable(
                        e.to_string(),
                    ));
                }
                // depth > 0: the note isn't witnessable at this older checkpoint
                // (appended after it, or the depth doesn't exist).
                Ok(None) => return Ok(None),
                // depth > 0: a genuine store failure. Log it so the operator sees
                // it (the anchor probe otherwise swallows the message), then treat
                // the depth as unusable — never a mid-probe abort.
                Err(e) => {
                    tracing::warn!(
                        position = *position,
                        depth,
                        error = %e,
                        "shielded anchor probe: witness_at_depth failed at depth > 0; skipping depth"
                    );
                    return Ok(None);
                }
            };

            // The anchor is derived from the witness path itself
            // (`MerklePath::root(cmx)`); all selected notes must agree on it, or
            // the store handed back witnesses from different checkpoints and the
            // spend builder would reject the mismatch downstream.
            let witness_anchor = merkle_path.root(*cmx);
            match &anchor {
                None => anchor = Some(witness_anchor),
                Some(prev) if prev.to_bytes() != witness_anchor.to_bytes() => {
                    return Err(PlatformWalletError::ShieldedBuildError(format!(
                        "witness anchor mismatch across selected notes (position {position})"
                    )));
                }
                _ => {}
            }

            spends.push(SpendableNote {
                note: *note,
                merkle_path,
            });
        }

        // `notes` is non-empty (the caller checked), so `anchor` is set.
        let anchor = anchor.ok_or_else(|| {
            PlatformWalletError::ShieldedBuildError(
                "no spendable notes selected — anchor undefined".to_string(),
            )
        })?;
        Ok(Some((spends, anchor)))
    };

    // Fast path: a fully-synced wallet's depth-0 root is a recorded anchor.
    let (spends, anchor) = match build_at_depth(0, true)? {
        Some(pair) => pair,
        // Unreachable — a strict build returns `Some` or errors — but stay
        // fund-safe (a clean error, never a panic) if that invariant breaks.
        None => {
            return Err(PlatformWalletError::ShieldedMerkleWitnessUnavailable(
                "depth-0 witness probe returned no witness for a selected note".to_string(),
            ));
        }
    };
    if recorded.contains(&anchor.to_bytes()) {
        return Ok((spends, anchor));
    }

    // Otherwise walk older checkpoints newest→oldest for the shallowest
    // recorded root.
    for depth in 1..MAX_ANCHOR_PROBE_DEPTH {
        match build_at_depth(depth, false)? {
            Some((spends, anchor)) if recorded.contains(&anchor.to_bytes()) => {
                return Ok((spends, anchor));
            }
            // A root exists at this depth but Platform didn't record it — try an
            // older checkpoint.
            Some(_) => continue,
            // A selected note isn't witnessable this deep; every deeper
            // checkpoint is older still, so none can cover it either.
            None => break,
        }
    }

    Err(PlatformWalletError::ShieldedNoRecordedAnchor(
        "no recorded anchor covers the selected notes; wait for the next shielded sync".to_string(),
    ))
}

/// Mark the selected notes as spent for `id`. Also queues a
/// shielded changeset on the persister so the spent flag reaches
/// durable storage immediately rather than waiting for the next
/// note scan to rediscover the spend (scan-based spend detection).
/// Also drops any matching pending reservation so the
/// confirmed-spent state and the in-flight-spend state can't
/// disagree.
async fn mark_notes_spent<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    notes: &[ShieldedNote],
) -> Result<(), PlatformWalletError> {
    let mut changeset = ShieldedChangeSet::default();
    {
        let mut store = store.write().await;
        for note in notes {
            if store
                .mark_spent(id, &note.nullifier)
                .map_err(|e| PlatformWalletError::ShieldedStoreError(e.to_string()))?
            {
                changeset.record_nullifier_spent(id, note.nullifier);
            }
        }
    }
    queue_shielded_changeset(persister, wallet_id, changeset);
    Ok(())
}

/// Select unspent notes and reserve them against an in-flight
/// spend in one write-locked critical section.
///
/// Combining selection and reservation under a single write lock
/// is the only thing that prevents two overlapping spend calls
/// from picking the same notes: with separate read-then-write
/// phases, the second caller would observe the same
/// `unspent_notes()` between the first caller's read and write
/// and proceed to build a duplicate proof that's only rejected
/// ~30 s later at broadcast time.
///
/// The reservation is held in memory until the spend saves its record
/// ([`save_spend_then_broadcast`]), which restores it after a restart: see
/// [`ShieldedStore::mark_pending`]. Callers must pair this with
/// [`finalize_pending`] (on proven execution) or [`settle_failed_spend`]
/// (on failure); until the notes are handed over
/// ([`NoteReservation::hand_over`]), the returned guard releases them when
/// the spend is dropped.
async fn reserve_unspent_notes<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    amount: u64,
    outputs: usize,
    fee_kind: ShieldedFeeKind,
    change: ChangeRequirement,
) -> Result<(Vec<ShieldedNote>, u64, u64, NoteReservation<S>), PlatformWalletError> {
    // All amount-based spends reject zero before waiting on a concurrent sync's store lock.
    ensure_positive_amount(amount)?;
    reserve_notes_covering(sdk, store, id, amount, outputs, fee_kind, change).await
}

/// Select and reserve, in one write-locked critical section, notes covering
/// `amount` plus the fee of `fee_kind`, which [`reserve_unspent_notes`] does
/// for a positive amount. A spend paying only its fee (an
/// `PrivateDocumentCreateFromShieldedPool`) reserves with an `amount` of 0.
/// Callers must pair this with [`finalize_pending`] / [`NoteReservation::cancel`], and
/// hand the guard over once they own the reservation.
async fn reserve_notes_covering<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    amount: u64,
    outputs: usize,
    fee_kind: ShieldedFeeKind,
    change: ChangeRequirement,
) -> Result<(Vec<ShieldedNote>, u64, u64, NoteReservation<S>), PlatformWalletError> {
    let mut guard = store.write().await;
    let unspent = guard
        .get_unspent_notes(id)
        .map_err(|e| PlatformWalletError::ShieldedStoreError(e.to_string()))?;
    let (selected, total_input, exact_fee) =
        select_notes_with_fee(&unspent, amount, outputs, fee_kind, change, sdk.version())?
            .into_owned();
    let reservation = mark_notes_pending(&mut *guard, store, id, &selected)?;
    Ok((selected, total_input, exact_fee, reservation))
}

/// Mark `notes` pending under the held store lock, returning the guard that
/// releases them unless handed over. A failure part way releases those
/// already marked.
fn mark_notes_pending<S: ShieldedStore>(
    locked: &mut S,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    notes: &[ShieldedNote],
) -> Result<NoteReservation<S>, PlatformWalletError> {
    let reservation = next_note_reservation();
    let mut marked = Vec::with_capacity(notes.len());
    for note in notes {
        if let Err(e) = locked.mark_pending_by(id, &note.nullifier, reservation) {
            release_reserved_notes(locked, id, &marked, reservation);
            return Err(PlatformWalletError::ShieldedStoreError(e.to_string()));
        }
        marked.push(note.nullifier);
    }
    Ok(NoteReservation {
        store: Arc::clone(store),
        id,
        nullifiers: marked,
        reservation,
        handed_over: false,
    })
}

/// A spend attempt's reservation id, unique in the process: what its note
/// reservations are owned by ([`ShieldedStore::mark_pending_by`]).
fn next_note_reservation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, AtomicOrdering::Relaxed)
}

/// Notes one spend reserved ([`ShieldedStore::mark_pending`]), released when
/// dropped before the spend hands them over ([`Self::hand_over`]): a spend
/// future dropped before its notes have an owner that frees them (an anchor
/// on each reservation, which a pruned anchor releases, or the outcome
/// handling that finalizes or cancels them) leaves them spendable again, as
/// [`NoteReservation::cancel`] would, rather than excluded for the rest of the
/// session. The release takes the store lock when it is free, and otherwise
/// runs on a spawned task once it frees; outside a runtime the notes stay
/// reserved until restart.
pub(super) struct NoteReservation<S: ShieldedStore> {
    store: Arc<RwLock<S>>,
    id: SubwalletId,
    nullifiers: Vec<[u8; 32]>,
    /// The attempt that owns the reservations: a release clears a note
    /// only while it still holds it, so a cleanup that runs after a purge,
    /// a rebind or a wallet's replacement never releases a newer attempt's
    /// notes or deletes its records.
    reservation: u64,
    handed_over: bool,
}

impl<S: ShieldedStore> NoteReservation<S> {
    /// The spend owns the reservation from now on.
    pub(super) fn hand_over(&mut self) {
        self.handed_over = true;
    }

    /// Own the reservation again, for a spend now known not to have executed: dropped from
    /// here on, the guard releases the notes, whatever owned them before.
    pub(super) fn reclaim(&mut self) {
        self.handed_over = false;
    }

    /// For a spend whose transition never left the wallet: retire its saved
    /// record `activity_id` with the journal records of `commitments` in one
    /// durable write ([`ShieldedStore::retire_undelivered_spend`]), and
    /// release the notes under the same store guard. Returns whether it was
    /// retired.
    ///
    /// When it was not (a redrive pass claimed a re-broadcast of the record,
    /// the record is gone, or the write failed), nothing changed and the
    /// notes stay reserved, handed over to the spend's resolution. So do they
    /// when the call is dropped while it waits for the store: the record may
    /// still be saved, and a note released under a saved transition could be
    /// spent twice.
    pub(super) async fn retire_undelivered(
        mut self,
        activity_id: &[u8; 32],
        commitments: &[[u8; 32]],
    ) -> bool {
        self.handed_over = true;
        let mut store = self.store.write().await;
        match store.retire_undelivered_spend(self.id, activity_id, commitments) {
            Ok(true) => {
                release_reserved_notes(&mut *store, self.id, &self.nullifiers, self.reservation);
                self.nullifiers.clear();
                true
            }
            Ok(false) => false,
            Err(e) => {
                warn!(
                    error = %e,
                    "a never-delivered spend's record was not retired; it is kept with its notes"
                );
                false
            }
        }
    }

    /// Release the notes now, for a spend that did not execute.
    /// The guard owns them until the release is done: a spend dropped while it waits for the
    /// store lock still releases them on drop.
    pub(super) async fn cancel(mut self) {
        self.handed_over = false;
        let mut store = self.store.write().await;
        release_reserved_notes(&mut *store, self.id, &self.nullifiers, self.reservation);
        self.nullifiers.clear();
    }
}

impl<S: ShieldedStore> Drop for NoteReservation<S> {
    fn drop(&mut self) {
        if self.handed_over || self.nullifiers.is_empty() {
            return;
        }
        let (id, reservation) = (self.id, self.reservation);
        let nullifiers = std::mem::take(&mut self.nullifiers);
        if let Ok(mut store) = self.store.try_write() {
            release_reserved_notes(&mut *store, id, &nullifiers, reservation);
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            warn!(
                "a dropped spend's notes were reserved outside a runtime; they stay until restart"
            );
            return;
        };
        let store = Arc::clone(&self.store);
        runtime.spawn(async move {
            let mut store = store.write().await;
            release_reserved_notes(&mut *store, id, &nullifiers, reservation);
        });
    }
}

/// Clear the reservations of `nullifiers` the spend attempt `reservation`
/// still holds ([`ShieldedStore::clear_pending_reserved_by`]).
fn release_reserved_notes<S: ShieldedStore>(
    store: &mut S,
    id: SubwalletId,
    nullifiers: &[[u8; 32]],
    reservation: u64,
) {
    for nullifier in nullifiers {
        if let Err(e) = store.clear_pending_reserved_by(id, nullifier, reservation) {
            warn!(
                error = %e,
                "releasing a dropped spend's note failed; the next note scan will reconcile"
            );
        }
    }
}

/// Exact-equality sibling of [`reserve_unspent_notes`] for
/// `IdentityCreateFromShieldedPool`: select + reserve notes covering exactly `denomination`
/// (the fee is metered FROM the denomination, not added to the target) in one write-locked
/// critical section, gating on `denomination > predicted_fee` via
/// [`select_notes_for_denomination`]. Returns the selected notes, total input value, and the
/// predicted fee. Callers must pair this with [`finalize_pending`] / [`NoteReservation::cancel`], and
/// hand the guard over once they own the reservation.
async fn reserve_unspent_notes_for_denomination<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    denomination: u64,
    num_outputs: usize,
    num_keys: usize,
) -> Result<(Vec<ShieldedNote>, u64, u64, NoteReservation<S>), PlatformWalletError> {
    // Reject zero or any other non-member before waiting on a concurrent sync's store lock.
    ensure_allowed_denomination(denomination, sdk.version())?;

    let mut guard = store.write().await;
    let unspent = guard
        .get_unspent_notes(id)
        .map_err(|e| PlatformWalletError::ShieldedStoreError(e.to_string()))?;
    let (selected, total_input, predicted_fee) = select_notes_for_denomination(
        &unspent,
        denomination,
        num_outputs,
        num_keys,
        sdk.version(),
    )?
    .into_owned();
    let reservation = mark_notes_pending(&mut *guard, store, id, &selected)?;
    Ok((selected, total_input, predicted_fee, reservation))
}

/// Promote a successful broadcast: mark the notes spent (which
/// also clears any matching pending reservation, see
/// [`SubwalletState::mark_spent`]) and queue the changeset for
/// the host persister.
async fn finalize_pending<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    notes: &[ShieldedNote],
) -> Result<(), PlatformWalletError> {
    mark_notes_spent(store, persister, wallet_id, id, notes).await
}

/// Run a synchronous build/prove `step` that executes strictly BEFORE any broadcast,
/// converting a panic into a definitive [`PlatformWalletError::ShieldedBuildError`].
///
/// Why: a panic that unwinds out of an operation skips its `match result` arms, so
/// `cancel_pending` never runs and the FFI boundary's panic guard has no choice but the
/// conservative ambiguous contract ("the spend may have broadcast — keep the reservation").
/// For a reservation that was never armed with an anchor that contract is a trap:
/// `stale_pending_spends` skips anchor-less reservations, so no later sync can release the
/// notes — they stay unselectable for the rest of the process lifetime, and the host is
/// explicitly told not to retry. A panic INSIDE the build/prove step, by contrast, is
/// provably pre-broadcast (the broadcast call sits strictly after it in the same operation),
/// so it is safe — and required — to convert it to a definitive error here, while the
/// operation still owns the selected notes: the operation's ordinary failure arm then
/// releases the reservation via `cancel_pending`. Panics during or after broadcast are NOT
/// caught by this helper; those remain genuinely ambiguous and keep the FFI guard's
/// conservative contract.
fn catch_pre_broadcast_panic<T>(
    operation: &'static str,
    step: impl FnOnce() -> T,
) -> Result<T, PlatformWalletError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step))
        .map_err(|payload| pre_broadcast_panic_error(operation, payload))
}

/// `async` sibling of [`catch_pre_broadcast_panic`] for an operation whose build/prove step is a
/// future (`identity_create_from_shielded_pool`, whose builder awaits the per-key
/// proof-of-possession signer).
///
/// Same contract, same conversion — only the shape of the guarded step differs. `catch_unwind`
/// cannot wrap a future's *polls* from the outside, so this drives the future through
/// `FutureExt::catch_unwind`, which applies the guard to every poll: a panic raised on any poll
/// of the builder (Halo 2 synthesis, note bookkeeping, a host signer callback) is caught while
/// the operation still owns the reservation. `AssertUnwindSafe` is the same assertion the
/// synchronous guard makes and is sound for the same reason: the caught panic is converted into
/// a definitive error whose failure arm RELEASES the shared state (the reservation) rather than
/// continuing to use it.
async fn catch_pre_broadcast_panic_async<T>(
    operation: &'static str,
    step: impl std::future::Future<Output = T>,
) -> Result<T, PlatformWalletError> {
    use futures::FutureExt;

    std::panic::AssertUnwindSafe(step)
        .catch_unwind()
        .await
        .map_err(|payload| pre_broadcast_panic_error(operation, payload))
}

/// The single panic-payload -> definitive-error conversion both pre-broadcast guards funnel
/// through, so the synchronous and asynchronous entry points cannot drift in the error they
/// produce (both must stay a `ShieldedBuildError`, the class
/// [`error_releases_note_reservation`] releases on).
fn pre_broadcast_panic_error(
    operation: &'static str,
    payload: Box<dyn std::any::Any + Send>,
) -> PlatformWalletError {
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string());
    PlatformWalletError::ShieldedBuildError(format!(
        "{operation}: build/prove panicked before broadcast: {message}"
    ))
}

/// Record the recorded `anchor` the spend was built against and the
/// linked activity entry on every selected note's reservation, so a
/// spend that ends broadcast-accepted-but-unconfirmed can be released
/// on a later sync once that anchor is pruned from Platform's recorded
/// set (see `NetworkShieldedCoordinator::release_stranded_spends`).
///
/// No-op when no activity entry was recorded (an output-less bundle —
/// unreachable for our own builders, which always carry a visible
/// output). A success (`finalize_pending`) or a definite failure
/// (`NoteReservation::cancel`) removes the entry, so only an ambiguous
/// unconfirmed outcome leaves it carrying the anchor.
///
/// Returns whether every reservation now carries the anchor, with
/// `anchor_height`, the proved height of the anchor set it was read from, so
/// an anchor proven pruned releases each. A store write that fails stops the spend before it
/// is broadcast ([`PlatformWalletError::ShieldedStoreError`]): the caller's
/// reservation guard still owns the notes, so they are released, rather
/// than a spend that may execute leaving reservations no pruned anchor
/// frees.
async fn arm_pending_release<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    anchor: [u8; 32],
    anchor_height: u64,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    notes: &[ShieldedNote],
) -> Result<bool, PlatformWalletError> {
    let Some(entry) = pending_entry else {
        return Ok(false);
    };
    let mut store = store.write().await;
    for note in notes {
        store
            .set_pending_spend(id, &note.nullifier, anchor, entry.id, Some(anchor_height))
            .map_err(|e| {
                PlatformWalletError::ShieldedStoreError(format!(
                    "the spend's note reservation could not carry its anchor, so it is not \
                     broadcast: {e}"
                ))
            })?;
    }
    Ok(true)
}

/// Maximum sync-time re-broadcast attempts for a spend whose outcome is
/// not known before the re-drive stops and the anchor-prune release
/// backstop owns the reservation.
pub(super) const MAX_REDRIVE_ATTEMPTS: u32 = 3;

/// Save a built shielded spend's recovery record, broadcast it and wait for
/// its result ([`broadcast_shielded_spend_with_redrive_with_metadata`]),
/// returning the proven result without its metadata.
#[allow(clippy::too_many_arguments)]
async fn broadcast_shielded_spend_with_redrive<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    anchor: [u8; 32],
    anchor_height: u64,
    notes: &[ShieldedNote],
    reservation: &mut NoteReservation<S>,
    state_transition: &StateTransition,
    operation: &'static str,
    wait: SpendResultWait,
) -> Result<StateTransitionProofResult, SpendFailure> {
    broadcast_shielded_spend_with_redrive_with_metadata(
        sdk,
        store,
        id,
        pending_entry,
        anchor,
        anchor_height,
        notes,
        reservation,
        state_transition,
        operation,
        wait,
    )
    .await
    .map(|(result, _)| result)
}

/// Save a built shielded spend's recovery record ([`PendingRedrive`])
/// durably, then broadcast it and wait for its result
/// ([`save_spend_then_broadcast`], [`broadcast_shielded_spend_classified`]).
///
/// The record is what resolves a spend whose outcome is not known: the next
/// scan detects a landing via the nullifiers; otherwise the byte-identical
/// transition is re-broadcast up to [`MAX_REDRIVE_ATTEMPTS`] times
/// (identical nullifiers cannot be spent twice); only if every attempt stays
/// silent does the anchor-prune release backstop take over. Saved before the
/// first dispatch, it also covers a process that exits during the dispatches
/// or the result wait: a restart restores the notes' reservation from it.
#[allow(clippy::too_many_arguments)]
async fn broadcast_shielded_spend_with_redrive_with_metadata<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    anchor: [u8; 32],
    anchor_height: u64,
    notes: &[ShieldedNote],
    reservation: &mut NoteReservation<S>,
    state_transition: &StateTransition,
    operation: &'static str,
    wait: SpendResultWait,
) -> Result<(StateTransitionProofResult, ResponseMetadata), SpendFailure> {
    save_spend_then_broadcast(
        store,
        id,
        pending_entry,
        anchor,
        anchor_height,
        notes,
        reservation,
        state_transition,
        operation,
        || broadcast_shielded_spend_classified(sdk, state_transition, operation, wait),
    )
    .await
}

/// Save the recovery record of the built note-consuming spend
/// `state_transition` durably, hand its notes over, and only then run
/// `broadcast`, which sends it and waits for its result: the production flows
/// pass their classified broadcast, a test intercepts it.
///
/// Each of `notes`' reservations first carries the spend's `anchor`
/// ([`arm_pending_release`]), so a pruned anchor releases it; the record is
/// then saved, keyed by `pending_entry` ([`arm_redrive_before_broadcast`]). A
/// spend with no entry to key its record by, or whose record cannot be saved,
/// is refused with nothing broadcast, and `reservation` still owns the notes:
/// the caller releases them ([`settle_failed_spend`]), and so does a spend
/// dropped while a save waits for the store.
///
/// Once the record is saved the notes are handed over, with nothing awaited
/// in between: from then on they belong to the spend's resolution, whatever
/// happens to this call. An exit or a drop during the dispatches or the
/// result wait leaves the reservation and the record, which a restart
/// restores and the redrive pass resolves.
#[allow(clippy::too_many_arguments)]
async fn save_spend_then_broadcast<S, B, Fut, T>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    anchor: [u8; 32],
    anchor_height: u64,
    notes: &[ShieldedNote],
    reservation: &mut NoteReservation<S>,
    state_transition: &StateTransition,
    operation: &'static str,
    broadcast: B,
) -> Result<T, SpendFailure>
where
    S: ShieldedStore,
    B: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, SpendFailure>>,
{
    arm_pending_release(store, id, anchor, anchor_height, pending_entry, notes).await?;
    arm_redrive_before_broadcast(
        store,
        id,
        pending_entry.as_ref().map(|entry| entry.id),
        anchor,
        notes.iter().map(|note| note.nullifier).collect(),
        state_transition,
        operation,
        None,
        None,
        anchor_height,
    )
    .await?;
    // Saved: the notes are the spend's resolution's from here on.
    reservation.hand_over();
    broadcast().await
}

/// Settle the notes, saved record and activity row of a note-consuming spend
/// (transfer, unshield, withdrawal, identity top-up, identity create) that
/// ended with `failure`, and return the error it reports.
///
/// - Its transition never left the wallet ([`SpendFailure::never_delivered`]:
///   refused before anything was sent), so nothing can execute it: its record
///   is retired and its notes are released under one store guard
///   ([`NoteReservation::retire_undelivered`]), and the activity row reads
///   failed. When the record cannot be retired (a redrive pass claimed a
///   re-broadcast of it meanwhile, or the write failed), the transition may
///   leave the wallet after all: everything stays and the spend reads as
///   unconfirmed.
/// - Its record was saved (`reservation` is handed over,
///   [`save_spend_then_broadcast`]) and a dispatch may have reached a node:
///   it may execute. The notes stay reserved and the record stays, for the
///   scan and the redrive pass to resolve, and the activity row stays
///   pending. It reads as unconfirmed, whatever error ended it.
/// - It failed before its record was saved: nothing was broadcast. The notes
///   are released and the activity row reads failed.
#[allow(clippy::too_many_arguments)]
async fn settle_failed_spend<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    persister: Option<&WalletPersister>,
    wallet_id: WalletId,
    id: SubwalletId,
    pending_entry: &Option<super::activity::ShieldedActivityEntry>,
    mut reservation: NoteReservation<S>,
    operation: &'static str,
    failure: SpendFailure,
) -> PlatformWalletError {
    let SpendFailure {
        error,
        never_delivered,
    } = failure;
    if never_delivered {
        let retired = match pending_entry.as_ref().map(|entry| entry.id) {
            Some(activity_id) => reservation.retire_undelivered(&activity_id, &[]).await,
            None => {
                reservation.hand_over();
                false
            }
        };
        if !retired {
            return PlatformWalletError::ShieldedSpendUnconfirmed {
                operation,
                reason: format!(
                    "its broadcast was refused before delivery ({error}), but its saved \
                     transition was claimed for a re-broadcast or could not be retired"
                ),
            };
        }
        record_activity_status(
            store,
            persister,
            wallet_id,
            id,
            pending_entry,
            ShieldedActivityStatus::Failed,
            None,
        )
        .await;
        return error;
    }
    if reservation.handed_over {
        return match error {
            unconfirmed @ (PlatformWalletError::ShieldedSpendUnconfirmed { .. }
            | PlatformWalletError::ShieldedBroadcastUnconfirmed { .. }) => unconfirmed,
            other => {
                // The reported error keeps the cause's text alone: its type
                // goes to the log.
                warn!(
                    account = id.account_index,
                    operation,
                    cause = ?other,
                    "Shielded spend ended with its record saved; reporting it unconfirmed"
                );
                PlatformWalletError::ShieldedSpendUnconfirmed {
                    operation,
                    reason: other.to_string(),
                }
            }
        };
    }
    // The guard owns the notes until they are released, so a drop on the
    // way still releases them.
    record_activity_status(
        store,
        persister,
        wallet_id,
        id,
        pending_entry,
        ShieldedActivityStatus::Failed,
        None,
    )
    .await;
    reservation.cancel().await;
    error
}

/// Save the re-drivable record of a spend durably before it is broadcast,
/// as identity-funded shields arm theirs: an exit after the node admitted
/// the transition, before its result returns, leaves the record. The record
/// is keyed by the spend's `activity_id` and carries its `nullifiers`, and
/// the private contact request the spend pays for when it does. A spend
/// paying for one is a private document create (type 29), and the private
/// payment journal keeps a record of each entry it appends, read from the
/// transition, in the same durable write ([`ShieldedStore::arm_private_contact_payment`]).
/// A spend whose record cannot be saved, or whose entries do not read, is
/// refused before anything is broadcast, as the unknown outcome could then
/// not be recovered. `payment_started`, when given, is marked right after
/// the record is saved, under the same store lock: dropped while it waits
/// for the lock, the spend leaves neither a record nor a mark.
/// `anchor_recorded_at` is the proved height of the recorded-anchor set the
/// spend's anchor was chosen from, kept in the journal record: an expiry
/// proof needs the anchor absent from a set proved at or above it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn arm_redrive_before_broadcast<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    activity_id: Option<[u8; 32]>,
    anchor: [u8; 32],
    nullifiers: Vec<[u8; 32]>,
    state_transition: &StateTransition,
    operation: &'static str,
    private_contact: Option<PrivateContactPayment>,
    payment_started: Option<&PaymentStarted>,
    anchor_recorded_at: u64,
) -> Result<(), PlatformWalletError> {
    use dpp::serialization::PlatformSerializable;

    let activity_id = activity_id.ok_or_else(|| {
        PlatformWalletError::ShieldedBuildError(format!(
            "{operation}: no activity entry keys the record of this spend, so it is not \
             broadcast"
        ))
    })?;
    let st_bytes = state_transition.serialize_to_bytes().map_err(|e| {
        PlatformWalletError::ShieldedBuildError(format!(
            "{operation}: the transition does not serialize for its record, so it is not \
             broadcast: {e}"
        ))
    })?;
    let journal = match private_contact {
        Some(payment) => Some((
            payment,
            private_document_entries(state_transition).ok_or_else(|| {
                PlatformWalletError::ShieldedBuildError(format!(
                    "{operation}: the entries this private contact request payment appends do \
                     not read, so it is not broadcast"
                ))
            })?,
        )),
        None => None,
    };
    let redrive = PendingRedrive {
        activity_id,
        anchor,
        nullifiers,
        st_bytes,
        attempts: 0,
        identity_nonce_finalized: false,
        identity_user_abandoned: false,
        anchor_height: Some(anchor_recorded_at),
        identity_index: None,
    };
    let mut store = store.write().await;
    match journal {
        Some((payment, entries)) => store.arm_private_contact_payment(
            id,
            redrive,
            payment,
            &entries,
            anchor_recorded_at,
            payment_started.and_then(PaymentStarted::store_position),
        ),
        None => store.arm_redrive(id, redrive),
    }
    .map_err(|e| {
        PlatformWalletError::ShieldedStoreError(format!(
            "{operation}: the record of this spend could not be saved, so it is not broadcast: \
             {e}"
        ))
    })?;
    if let Some(payment_started) = payment_started {
        payment_started.mark();
    }
    Ok(())
}

/// Discharge `wallet_id`'s journal records of the private contact request
/// entries a private sync read back and recorded durably, as `persister`
/// noted them (`WalletPersister::note_read_back_private_entries`). One whose
/// discharge fails is noted again for the next call.
pub(crate) fn discharge_read_back_private_payments<S: ShieldedStore>(
    store: &mut S,
    persister: &WalletPersister,
    wallet_id: WalletId,
) {
    let mut kept = Vec::new();
    let mut discharged = Vec::new();
    for commitment in persister.take_read_back_private_entries() {
        match store.discharge_private_payment(wallet_id, &commitment) {
            Ok(_) => discharged.push(commitment),
            Err(e) => {
                warn!(
                    error = %e,
                    "a read-back private payment's journal record was not discharged; retried \
                     later"
                );
                kept.push(commitment);
            }
        }
    }
    persister.note_discharged_private_entries(discharged);
    if !kept.is_empty() {
        persister.note_read_back_private_entries(kept);
    }
}

/// [`private_document_commitments`], each with the SHA-256 of its whole entry,
/// as a private contact request payment's journal records keep them.
pub(crate) fn private_document_entries(
    state_transition: &StateTransition,
) -> Option<Vec<PrivatePaymentEntry>> {
    use dpp::state_transition::private_document_create_from_shielded_pool_transition::accessors::PrivateDocumentCreateFromShieldedPoolTransitionAccessorsV0;

    let StateTransition::PrivateDocumentCreateFromShieldedPool(transition) = state_transition
    else {
        return None;
    };
    let commitments = private_document_commitments(state_transition)?;
    Some(
        commitments
            .into_iter()
            .zip(transition.entries())
            .map(|(commitment, entry)| PrivatePaymentEntry {
                commitment,
                entry_hash: Sha256::digest(entry.as_slice()).into(),
            })
            .collect(),
    )
}

/// The commitments of the entries the private document create (type 29)
/// `state_transition` appends, each entry's first 32 bytes; `None` for
/// another transition, one appending no entry, or an entry shorter than its
/// commitment.
pub(crate) fn private_document_commitments(
    state_transition: &StateTransition,
) -> Option<Vec<[u8; 32]>> {
    use dpp::state_transition::private_document_create_from_shielded_pool_transition::accessors::PrivateDocumentCreateFromShieldedPoolTransitionAccessorsV0;

    let StateTransition::PrivateDocumentCreateFromShieldedPool(transition) = state_transition
    else {
        return None;
    };
    if transition.entries().is_empty() {
        return None;
    }
    transition
        .entries()
        .iter()
        .map(|entry| {
            entry
                .as_slice()
                .get(..32)
                .and_then(|commitment| <[u8; 32]>::try_from(commitment).ok())
        })
        .collect()
}

/// Whether an SDK error is Platform's `NullifierAlreadySpentError` —
/// on a RE-broadcast of our own byte-identical transition this means
/// the ORIGINAL broadcast executed: the nullifiers are consumed by the
/// very spend being re-driven, so it is a success signal (the next scan
/// confirms the notes spent), never a failure.
fn is_nullifier_already_spent(e: &dash_sdk::Error) -> bool {
    use dpp::consensus::state::state_error::StateError;
    use dpp::consensus::ConsensusError;

    let consensus: Option<&ConsensusError> = match e {
        dash_sdk::Error::Protocol(dpp::ProtocolError::ConsensusError(c)) => Some(c),
        dash_sdk::Error::StateTransitionBroadcastError(b) => b.cause.as_ref(),
        _ => None,
    };
    matches!(
        consensus,
        Some(ConsensusError::StateError(
            StateError::NullifierAlreadySpentError(_)
        ))
    )
}

/// Pure outcome classification for one re-broadcast attempt. Extracted
/// from the redrive loop so the arm ORDER — `AlreadyExecuted` must win
/// over the generic consensus-rejection check, since
/// `NullifierAlreadySpent` is itself a consensus error — is pinned by
/// unit tests without needing a broadcast-mockable network seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedriveBroadcastOutcome {
    /// Relay accepted the re-broadcast; the next scan detects a landing.
    Accepted,
    /// `NullifierAlreadySpent`: the ORIGINAL broadcast executed — a
    /// success signal, never a failure.
    AlreadyExecuted,
    /// Any other outcome, a consensus verdict included: the original's
    /// broadcast was ambiguous, so a refusal of the re-broadcast does not
    /// prove it can never execute.
    Inconclusive,
}

fn classify_redrive_broadcast(result: &Result<(), dash_sdk::Error>) -> RedriveBroadcastOutcome {
    match result {
        Ok(()) => RedriveBroadcastOutcome::Accepted,
        Err(e) if is_nullifier_already_spent(e) => RedriveBroadcastOutcome::AlreadyExecuted,
        Err(_) => RedriveBroadcastOutcome::Inconclusive,
    }
}

/// Bump a redrive attempt counter, logging (rather than discarding) a
/// persistence failure. On `Err` the durable counter did not advance and
/// the file store's persist-first ordering leaves memory untouched, so
/// the same attempt slot is retried on the next pass — the log line is
/// what makes that visible.
async fn bump_redrive_attempts_logged<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    activity_id: &[u8; 32],
) -> u32 {
    match store.write().await.bump_redrive_attempts(id, activity_id) {
        Ok(attempts) => attempts,
        Err(e) => {
            warn!(
                error = %e,
                "redrive: failed to persist the attempt counter; the attempt will be \
                 retried on the next pass"
            );
            0
        }
    }
}

/// Claim one re-broadcast of `redrive`, durably, from the record as it is now,
/// under the store write lock a never-delivered spend's retirement takes
/// ([`ShieldedStore::retire_undelivered_spend`]): the attempt is counted
/// before the broadcast starts, so a retirement after the claim sees it and
/// refuses. A record retired, armed again with other bytes, or out of attempts
/// since `redrive` was read is not claimed, and not broadcast. Returns the
/// attempt count, or `None` when nothing was claimed: a counter that fails to
/// persist claims nothing.
async fn claim_redrive_attempt<S: ShieldedStore>(
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    redrive: &PendingRedrive,
) -> Option<u32> {
    let mut store = store.write().await;
    let live = store.pending_redrives(id).is_ok_and(|current| {
        current.iter().any(|record| {
            record.activity_id == redrive.activity_id
                && record.st_bytes == redrive.st_bytes
                && record.attempts < MAX_REDRIVE_ATTEMPTS
        })
    });
    if !live {
        debug!("redrive: the record was retired or changed since it was read; not re-broadcast");
        return None;
    }
    match store.bump_redrive_attempts(id, &redrive.activity_id) {
        Ok(attempts) if attempts > 0 => Some(attempts),
        Ok(_) => None,
        Err(e) => {
            warn!(
                error = %e,
                "redrive: the attempt could not be counted, so it is not re-broadcast; the next \
                 pass retries"
            );
            None
        }
    }
}

/// Re-broadcast every unresolved ShieldFromIdentity transition for `wallet_id`.
/// These records have no nullifiers or spending anchor, so they must not depend
/// on the note-spend stranded-release pass. Exact-byte retries remain safe even
/// after the ordinary spend attempt budget is exhausted: the fixed identity
/// nonce prevents the same transition from debiting twice, while continuing
/// retries lets a wallet recover after switching away from a failing endpoint.
pub(super) async fn redrive_pending_identity_shields<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    wallet_id: WalletId,
    cancel: &CancellationToken,
) {
    use dpp::serialization::PlatformDeserializableTrusted;
    use dpp::state_transition::shield_from_identity_transition::accessors::ShieldFromIdentityTransitionAccessorsV0;

    let redrives = match store.read().await.pending_redrives_for_wallet(wallet_id) {
        Ok(redrives) => redrives,
        Err(e) => {
            warn!(error = %e, "identity redrive: guard lookup failed");
            return;
        }
    };
    for (id, redrive) in redrives {
        // A stop ends the pass between records: no new request starts
        // once cancelled. A request already on the wire is left to
        // finish on its own (its outcome is handled conservatively).
        if cancel.is_cancelled() {
            debug!("identity redrive: pass cancelled; remaining guards wait for the next pass");
            return;
        }
        if !redrive.nullifiers.is_empty()
            || redrive.identity_nonce_finalized
            || redrive.identity_user_abandoned
        {
            continue;
        }
        let state_transition =
            match StateTransition::deserialize_from_bytes_trusted(&redrive.st_bytes) {
                Ok(state_transition @ StateTransition::ShieldFromIdentity(_)) => state_transition,
                _ => {
                    warn!("identity redrive: unreadable debit; retaining its retry guard");
                    continue;
                }
            };
        let StateTransition::ShieldFromIdentity(identity_transition) = &state_transition else {
            continue;
        };
        // The nonce read is the one wait here a stop can cut short: it
        // is read-only, so dropping it mid-flight loses nothing.
        let Some(fetched) = cancel
            .run_until_cancelled(IdentityNonceFetcher::fetch(
                sdk,
                identity_transition.identity_id(),
            ))
            .await
        else {
            debug!("identity redrive: pass cancelled during the nonce read");
            return;
        };
        if let Ok(fetched) = fetched {
            let result = {
                let mut store = store.write().await;
                reconcile_identity_shield_nonce(
                    &mut *store,
                    id,
                    redrive.activity_id,
                    fetched.map(|fetcher| fetcher.0),
                    identity_transition.nonce(),
                )
            };
            match result {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    warn!(error = %e, "identity redrive: could not persist nonce finality");
                    continue;
                }
            }
        }
        // A host recovery decision may have archived this snapshot while the
        // nonce request was in flight. Do not initiate a new broadcast from it.
        // Already submitted network requests cannot be cancelled by this check.
        let still_active = store
            .read()
            .await
            .pending_redrives(id)
            .map(|current| {
                current.iter().any(|record| {
                    record.activity_id == redrive.activity_id
                        && record.st_bytes == redrive.st_bytes
                        && !record.identity_user_abandoned
                        && !record.identity_nonce_finalized
                })
            })
            .unwrap_or(false);
        if !still_active {
            continue;
        }
        // Last check before a broadcast starts; a broadcast already
        // submitted cannot be cancelled by this check.
        if cancel.is_cancelled() {
            debug!("identity redrive: pass cancelled before the re-broadcast");
            return;
        }
        // Unused or unverified state permits only the original signed bytes.
        // Even an apparently definitive rejection cannot release the guard:
        // this request, or an earlier relay, may deliver the debit afterward.
        if let Err(e) = state_transition.broadcast(sdk, None).await {
            debug!(error = %e, "identity redrive: original payment remains pending");
        }
        bump_redrive_attempts_logged(store, id, &redrive.activity_id).await;
    }
}

/// Sync-time re-drive for `id`'s armed unconfirmed spends: for each
/// [`PendingRedrive`] whose anchor is still in Platform's `recorded`
/// set and whose attempt budget remains, re-broadcast the stored
/// byte-identical transition (relay-ACK only — the landing itself is
/// detected by the NEXT scan's nullifier reconcile) and classify:
///
/// - accepted / inconclusive → count the attempt; wait for the next scan;
/// - `NullifierAlreadySpent` → the original executed; the next scan
///   confirms — touch nothing;
/// - any other consensus verdict → inconclusive too: a node refusing the
///   re-broadcast does not show that no other node holds the original,
///   whose broadcast was ambiguous, so the notes stay reserved;
/// - pruned anchor / exhausted attempts → leave it to the
///   prune-backstop release pass, which releases the notes once the
///   anchor is pruned and the spend can never execute.
///
/// A record whose transition does not decode is kept, never dropped: it
/// may be the only evidence of a payment that can still execute. Its notes
/// stay reserved until the spend lands or the prune backstop releases them.
///
/// Runs after the scan's spent-note reconcile (a landed spend's record
/// was already dropped by the `mark_spent` hook, so anything still
/// armed here is genuinely unresolved).
pub(super) async fn redrive_pending_spends<S: ShieldedStore>(
    sdk: &Arc<dash_sdk::Sdk>,
    store: &Arc<RwLock<S>>,
    id: SubwalletId,
    recorded: &std::collections::HashSet<[u8; 32]>,
    cancel: &CancellationToken,
) {
    use dpp::serialization::PlatformDeserializableTrusted;

    let redrives = match store.read().await.pending_redrives(id) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                error = %e,
                "redrive: pending_redrives failed; skipping subwallet"
            );
            return;
        }
    };
    for redrive in redrives {
        // A stop ends the pass between records: no new re-broadcast starts
        // once cancelled. One already submitted is left to finish and is
        // classified as usual — its outcome handling stays conservative.
        if cancel.is_cancelled() {
            debug!("redrive: pass cancelled; remaining records wait for the next pass");
            return;
        }
        // Empty-nullifier identity debits are wallet-wide and are driven by
        // `redrive_pending_identity_shields`, independently of spending anchors.
        if redrive.nullifiers.is_empty() {
            continue;
        }
        // A pruned anchor is the release pass's call, not ours; an
        // exhausted budget means we've said our three pieces.
        if !recorded.contains(&redrive.anchor) || redrive.attempts >= MAX_REDRIVE_ATTEMPTS {
            continue;
        }
        let st = match StateTransition::deserialize_from_bytes_trusted(&redrive.st_bytes) {
            Ok(st) => st,
            Err(e) => {
                warn!(
                    error = %e,
                    "redrive: stored transition failed to deserialize; keeping the record \
                     without re-broadcasting it (the prune backstop still frees the notes)"
                );
                continue;
            }
        };
        let Some(attempts) = claim_redrive_attempt(store, id, &redrive).await else {
            continue;
        };
        let broadcast_result = st.broadcast(sdk, None).await;
        let err_display = broadcast_result
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        match classify_redrive_broadcast(&broadcast_result) {
            RedriveBroadcastOutcome::Accepted => {
                info!(
                    attempts,
                    max = MAX_REDRIVE_ATTEMPTS,
                    "redrive: re-broadcast accepted; the next scan detects the landing"
                );
            }
            RedriveBroadcastOutcome::AlreadyExecuted => {
                // Success signal, but the attempt still counts: the scan
                // normally confirms the landing and drops the record, and
                // if it lags, this arm must not re-broadcast unboundedly
                // on every pass. The cap parks the record for the scan /
                // prune passes to settle.
                info!(
                    attempts,
                    max = MAX_REDRIVE_ATTEMPTS,
                    "redrive: transition already executed on-chain; the next scan confirms \
                     the notes spent"
                );
            }
            RedriveBroadcastOutcome::Inconclusive => {
                // `AlreadyExists` (still in a mempool after a lost-ACK
                // retry) or transport noise: inconclusive; counts toward
                // the cap.
                debug!(
                    attempts,
                    max = MAX_REDRIVE_ATTEMPTS,
                    error = %err_display,
                    "redrive: re-broadcast inconclusive"
                );
            }
        }
    }
}

/// How [`broadcast_shielded_spend_classified`] waits for an already-broadcast
/// spend's result, mirroring the verifier's classification of the family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpendResultWait {
    /// The family is classified execution-proving (transfer, unshield,
    /// withdrawal): a snapshot outcome is an error.
    ExecutionProved,
    /// The proof only authenticates the affected keys' state at the proof's
    /// block (identity top-up: the spent nullifiers are shared with any
    /// competing spend of the same notes and the credited identity's balance
    /// is a snapshot). Accepted as that snapshot.
    AffectedState,
}

/// A failed note-consuming spend: the error it reports, and whether its
/// transition never left the wallet. Every failure before the broadcast, and
/// every one after a dispatch that may have reached a node, is built from its
/// error alone ([`From`]), with `never_delivered` false.
struct SpendFailure {
    error: PlatformWalletError,
    /// The broadcast proved nothing was delivered
    /// ([`BroadcastDelivery::NeverDelivered`]): nothing holds the transition's
    /// bytes but the wallet, so it can never execute.
    never_delivered: bool,
}

impl From<PlatformWalletError> for SpendFailure {
    fn from(error: PlatformWalletError) -> Self {
        Self {
            error,
            never_delivered: false,
        }
    }
}

/// Broadcast a built shielded spend transition and wait for its proven
/// result, telling a failure whose transition never left the wallet from one
/// that may have reached the network:
///
/// - refused before anything was sent ([`BroadcastDelivery::NeverDelivered`]):
///   [`PlatformWalletError::ShieldedBroadcastFailed`], with
///   [`SpendFailure::never_delivered`] set, so the caller retires the saved
///   record and releases the notes;
/// - any dispatch that may have reached a node falls through to the result
///   wait, which either proves the outcome or leaves it unknown;
/// - any wait failure, a consensus verdict included
///   ([`classify_spend_wait_failure`]):
///   [`PlatformWalletError::ShieldedSpendUnconfirmed`], and the caller leaves
///   the reservations and the saved record in place.
///
/// A spend leaves no artifact as cheaply queryable as the identity row
/// [`broadcast_identity_create`] falls back to, so an unknown outcome is
/// surfaced directly and resolved by the next nullifier sync and the redrive
/// pass.
async fn broadcast_shielded_spend_classified(
    sdk: &Arc<dash_sdk::Sdk>,
    state_transition: &StateTransition,
    operation: &'static str,
    wait: SpendResultWait,
) -> Result<(StateTransitionProofResult, ResponseMetadata), SpendFailure> {
    match broadcast_tracking_delivery(sdk, state_transition).await {
        BroadcastDelivery::Admitted => {}
        BroadcastDelivery::NeverDelivered(e) => {
            return Err(SpendFailure {
                error: definite_broadcast_failure(&e),
                never_delivered: true,
            });
        }
        BroadcastDelivery::MaybeDelivered(e) => {
            warn!(
                operation,
                error = %e,
                "Shielded spend broadcast may have been delivered; falling through to the \
                 result wait"
            );
        }
    }

    // The verifier's classification of the family decides the wait: a
    // proof-binding family rejects a snapshot outcome, an affected-state family
    // accepts it. Either way the proven result is returned so callers that
    // credit a known identity can apply its proof-attested balance.
    let waited = match wait {
        SpendResultWait::ExecutionProved => {
            state_transition
                .wait_for_response_with_metadata::<StateTransitionProofResult>(sdk, None)
                .await
        }
        SpendResultWait::AffectedState => {
            state_transition
                .wait_for_affected_state_with_metadata::<StateTransitionProofResult>(sdk, None)
                .await
        }
    };
    waited
        .map_err(|wait_err| classify_spend_wait_failure(operation, &wait_err))
        .map_err(SpendFailure::from)
}

/// The error a spend whose transition never left the wallet
/// ([`BroadcastDelivery::NeverDelivered`], refused before anything was sent)
/// is reported as: a failed broadcast. Its notes are released. Every
/// broadcast that reached an endpoint waits for its result instead, and
/// reads as unconfirmed when that wait fails.
fn definite_broadcast_failure(e: &dash_sdk::Error) -> PlatformWalletError {
    PlatformWalletError::ShieldedBroadcastFailed(e.to_string())
}

/// On-chain spent status of a claim's nullifier set, as far as a single
/// query can establish it.
///
/// The three states matter because callers draw OPPOSITE conclusions from
/// them: `Spent` proves the invitation notes are consumed (something
/// executed), `Unspent` proves nothing has consumed them yet, and
/// `Unknown` proves NOTHING — a transport failure or an absent response
/// must never be read as either of the other two (#4204 review finding
/// 8d020115b274).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NullifierSpentStatus {
    /// At least one queried nullifier is proof-verified spent.
    Spent,
    /// The query succeeded and covered every queried nullifier; none is spent.
    Unspent,
    /// The query failed, returned no response, or covered only part of the
    /// queried set — no conclusion can be drawn.
    Unknown,
}

/// Classify a successful nullifier-status response against the queried set.
///
/// A response that omits some queried nullifiers proves nothing about the
/// omitted ones, so it downgrades an all-unspent answer to `Unknown`.
fn classify_nullifier_statuses(
    statuses: &[dash_sdk::query_types::ShieldedNullifierStatus],
    queried: &[[u8; 32]],
) -> NullifierSpentStatus {
    if statuses.iter().any(|s| s.is_spent) {
        return NullifierSpentStatus::Spent;
    }
    let covered = queried
        .iter()
        .all(|q| statuses.iter().any(|s| &s.nullifier == q));
    if covered {
        NullifierSpentStatus::Unspent
    } else {
        NullifierSpentStatus::Unknown
    }
}

/// On-chain check: are `nullifiers` already recorded spent in Platform's
/// shielded nullifier set? Reuses the proof-verified
/// [`ShieldedNullifierStatuses`](dash_sdk::query_types::ShieldedNullifierStatuses)
/// fetch (query type [`ShieldedNullifiersQuery`](dash_sdk::query_types::ShieldedNullifiersQuery)).
///
/// A query error or an absent response is [`NullifierSpentStatus::Unknown`],
/// never `Unspent`: the pre-broadcast preflight may treat unknown as
/// "proceed" (the idempotent broadcast path reconciles via the
/// `NullifierAlreadySpent` verdict, so that only costs a harmless rebuild),
/// but the post-verdict classification must NOT — declaring a definitive
/// non-execution on an unknown status would report an applied chargeable
/// fallback as retryable.
async fn nullifier_spent_status(
    sdk: &Arc<dash_sdk::Sdk>,
    nullifiers: &[[u8; 32]],
) -> NullifierSpentStatus {
    use dash_sdk::platform::Fetch;
    use dash_sdk::query_types::{ShieldedNullifierStatuses, ShieldedNullifiersQuery};

    if nullifiers.is_empty() {
        return NullifierSpentStatus::Unspent;
    }
    match ShieldedNullifierStatuses::fetch(sdk, ShieldedNullifiersQuery(nullifiers.to_vec())).await
    {
        Ok(Some(statuses)) => classify_nullifier_statuses(&statuses.0, nullifiers),
        Ok(None) => NullifierSpentStatus::Unknown,
        Err(e) => {
            warn!(
                error = %e,
                "IdentityCreateFromOneTimeKey: nullifier spent-status query failed; status unknown"
            );
            NullifierSpentStatus::Unknown
        }
    }
}

/// The 20-byte hash of the MASTER authentication key among `public_keys`
/// (`purpose = AUTHENTICATION`, `security_level = MASTER`). This is the unique,
/// Platform-indexed key hash an identity can be looked up by — the exact probe
/// [`IdentityWallet::discover_inner`] scans with
/// (`Identity::fetch(sdk, PublicKeyHash(..))`). The invitee re-derives these
/// same creation keys from its own seed on a retry, so this hash re-derives
/// deterministically and needs no persisted record.
fn master_auth_public_key_hash(
    public_keys: &[(IdentityPublicKey, IdentityPublicKeyInCreation)],
) -> Option<[u8; 20]> {
    master_auth_public_key_hash_of(public_keys.iter().map(|(key, _)| key))
}

/// [`master_auth_public_key_hash`] over any borrowed key sequence — used by the
/// resume path, whose keys come out of the stored transition rather than out of
/// the caller's `(IdentityPublicKey, IdentityPublicKeyInCreation)` pairs.
fn master_auth_public_key_hash_of<'a>(
    public_keys: impl IntoIterator<Item = &'a IdentityPublicKey>,
) -> Option<[u8; 20]> {
    use dpp::identity::identity_public_key::methods::hash::IdentityPublicKeyHashMethodsV0;
    use dpp::identity::{Purpose, SecurityLevel};

    public_keys
        .into_iter()
        .find(|key| {
            key.purpose() == Purpose::AUTHENTICATION
                && key.security_level() == SecurityLevel::MASTER
        })
        .and_then(|key| key.public_key_hash().ok())
}

/// Positive evidence that `identity` was created by **this** claim's Type-20
/// transition.
///
/// Two independent bindings must BOTH hold. Each one alone is satisfied by a
/// real on-chain outcome in which this claim did *not* create the identity, so
/// neither is sufficient on its own:
///
/// 1. **Id binding** — `identity.id()` equals `expected_identity_id`, the id
///    derived from this claim's published spend nullifiers
///    (`identity_id_from_nullifiers`). Consensus re-derives the id the same way
///    and rejects any transition whose declared id differs (see
///    `derive_identity_id_from_actions` in the Type-20 state validation), so an
///    identity carrying this id can only have been created by a transition that
///    published exactly this claim's nullifier set.
///
///    Without it, the MASTER-key-hash lookup accepts the **pre-existing**
///    identity that a chargeable `UnshieldAction` fallback collided with: when a
///    submitted unique key hash is already registered, Type-20 finalizes the
///    spend as an `UnshieldTransitionAction` (`chargeable_failure: true`) and
///    creates no identity, yet the nullifier is consumed and the colliding
///    identity *is* findable under our own key hash.
///
/// 2. **Key binding** — the identity's **on-chain** key set contains this
///    claim's submitted MASTER authentication key hash.
///
///    Without it, the derived-id lookup accepts an identity created by a
///    *different* holder of the same bearer one-time key: the id is derived from
///    nullifiers only, never from identity keys, so two holders racing the same
///    invitation derive the same id under different keys.
///
/// The key binding is checked against the keys the fetch actually returned — an
/// identity that comes back without public keys fails closed rather than being
/// topped up with locally-submitted keys that were never proven to exist on
/// chain.
///
/// `expected_identity_id == None` means the id is not re-derivable for this
/// claim, so binding 1 cannot be established and this returns `false`. That is
/// the single-spend case: `BundleType::DEFAULT` pads a one-action bundle to
/// Orchard's 2-action minimum and the padding action's **randomly generated**
/// dummy nullifier participates in the id derivation, so a retry cannot
/// reproduce the original id.
fn recovered_identity_matches_claim(
    identity: &Identity,
    expected_identity_id: Option<Identifier>,
    master_key_hash: Option<[u8; 20]>,
) -> bool {
    use dpp::identity::identity_public_key::methods::hash::IdentityPublicKeyHashMethodsV0;
    use dpp::identity::{Purpose, SecurityLevel};

    // Both handles must be available; a missing one is not evidence.
    let (Some(expected_id), Some(expected_hash)) = (expected_identity_id, master_key_hash) else {
        return false;
    };

    // Binding 1: the id must be the one derived from this claim's nullifiers.
    if identity.id() != expected_id {
        return false;
    }

    // Binding 2: the on-chain key set must carry this claim's MASTER auth key.
    identity.public_keys().values().any(|key| {
        key.purpose() == Purpose::AUTHENTICATION
            && key.security_level() == SecurityLevel::MASTER
            && key
                .public_key_hash()
                .is_ok_and(|hash| hash == expected_hash)
    })
}

/// Recover the identity a claim created by looking it up under its MASTER auth
/// key hash, with the same bounded retry cadence as
/// [`fetch_identity_with_retries`] to ride out DAPI indexing lag. Reuses
/// `discover_inner`'s unique-hash primitive (`Identity::fetch(sdk,
/// PublicKeyHash(..))`).
async fn fetch_identity_by_key_hash_with_retries(
    sdk: &Arc<dash_sdk::Sdk>,
    key_hash: [u8; 20],
) -> Option<Identity> {
    use dash_sdk::platform::types::identity::PublicKeyHash;
    use dash_sdk::platform::Fetch;

    for attempt in 0..IDENTITY_CREATE_FETCH_RETRIES {
        match Identity::fetch(sdk, PublicKeyHash(key_hash)).await {
            Ok(Some(identity)) => return Some(identity),
            Ok(None) => {
                trace!(
                    key_hash = %hex::encode(key_hash),
                    attempt,
                    "IdentityCreateFromOneTimeKey recovery: identity not found by key hash yet"
                );
            }
            Err(e) => {
                trace!(
                    key_hash = %hex::encode(key_hash),
                    attempt,
                    error = %e,
                    "IdentityCreateFromOneTimeKey recovery: key-hash lookup errored; will retry"
                );
            }
        }
        if attempt + 1 < IDENTITY_CREATE_FETCH_RETRIES {
            tokio::time::sleep(IDENTITY_CREATE_FETCH_RETRY_DELAY).await;
        }
    }
    None
}

/// This one-time-key claim's note is already spent on chain (the spent-nullifier
/// preflight saw it, or the broadcast/wait returned `NullifierAlreadySpent`).
/// Decide what that actually means and return the matching outcome.
///
/// A spent nullifier proves only that *something* consumed the invitation note —
/// **not** that this claim created an identity. Type-20 also consumes the note on
/// its chargeable `UnshieldAction` fallback, which creates no identity at all.
/// So every candidate identity found here must clear both ownership bindings in
/// [`recovered_identity_matches_claim`] before it can be reported as this
/// claim's result.
///
/// Two lookup handles are tried, each with bounded retries for DAPI indexing lag:
/// 1. the invitee's MASTER auth key hash (`discover_inner`'s unique-hash probe),
/// 2. the id derived from this claim's published nullifiers.
///
/// Outcomes:
/// - **`Ok`** — a fetched identity cleared both bindings: this claim created it.
/// - **[`PlatformWalletError::ShieldedInviteAlreadyClaimed`]** — an identity was
///   fetched but failed a binding (chargeable fallback, a competing holder of
///   the same bearer key, or — when `master_key_hash` is `None` — a key binding
///   that can never be established for this claim), *or* the id is not
///   re-derivable so no binding can ever be established. Terminal: the note is
///   spent, so retrying cannot help.
/// - **[`PlatformWalletError::ShieldedBroadcastUnconfirmed`]** — nothing resolved
///   yet, but the id *is* re-derivable, so a later retry can still reconcile once
///   indexing catches up. Only reachable when `expected_identity_id` is `Some`,
///   so the carried id is always the one this claim's nullifiers derive.
///
/// `spend_finalized` — the caller holds POSITIVE evidence that this claim's own
/// broadcast reached a definitive consensus verdict AND the notes are proven
/// consumed. Under that evidence, "no identity carries this claim's bindings"
/// is not indexing lag: an applied Type-20 that returned an error verdict
/// created no identity (the chargeable `UnshieldAction` fallback), and the
/// colliding unique key need not be MASTER — a collision on any other submitted
/// unique key leaves NOTHING findable under the MASTER-hash probe or the
/// derived id. The nothing-found outcome is then the terminal
/// `ShieldedInviteAlreadyClaimed`, not `ShieldedBroadcastUnconfirmed`
/// (#4204 review finding 8d020115b274).
async fn recover_executed_one_time_claim(
    sdk: &Arc<dash_sdk::Sdk>,
    master_key_hash: Option<[u8; 20]>,
    expected_identity_id: Option<Identifier>,
    spend_finalized: bool,
    evidence: &str,
) -> Result<(Identifier, Identity), PlatformWalletError> {
    warn!(
        ?expected_identity_id,
        evidence,
        "IdentityCreateFromOneTimeKey: invitation note already spent on chain; checking whether \
         this claim actually created an identity"
    );

    // The id is not re-derivable (single-spend bundle padded with a random dummy
    // nullifier), so no candidate identity can ever be bound to this claim.
    // Report the invitation as claimed rather than inventing a success.
    let Some(expected_id) = expected_identity_id else {
        return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
            reason: format!(
                "the note was spent by an earlier transition whose identity id cannot be \
                 re-derived (single-spend bundles are padded with a randomly generated dummy \
                 nullifier that participates in the id derivation): {evidence}"
            ),
        });
    };

    // Handle 1: the invitee's own MASTER auth key hash.
    if let Some(key_hash) = master_key_hash {
        if let Some(identity) = fetch_identity_by_key_hash_with_retries(sdk, key_hash).await {
            if recovered_identity_matches_claim(&identity, expected_identity_id, master_key_hash) {
                info!(
                    identity_id = %identity.id(),
                    "IdentityCreateFromOneTimeKey: recovered this claim's identity by its master \
                     auth key hash (id and key bindings both verified)"
                );
                return Ok((identity.id(), identity));
            }
            // Found under our key hash but NOT created by this claim — the
            // chargeable-`UnshieldAction` outcome: the spend was finalized, the
            // value went to the fallback address, and this pre-existing identity
            // merely owns the colliding key hash.
            warn!(
                found_id = %identity.id(),
                expected_id = %expected_id,
                "IdentityCreateFromOneTimeKey: an identity owns this claim's master auth key hash \
                 but its id is not the one this claim's nullifiers derive; the spend was finalized \
                 as a chargeable failure and created no identity"
            );
            return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
                reason: format!(
                    "identity {} owns the submitted master auth key hash but was not created by \
                     this claim (expected id {}); the shielded spend was finalized as a chargeable \
                     failure and its value went to the creation-failure address: {evidence}",
                    identity.id(),
                    expected_id
                ),
            });
        }
    }

    // Handle 2: the id derived from this claim's published nullifiers.
    if let Some(identity) = fetch_identity_with_retries(sdk, expected_id).await {
        if recovered_identity_matches_claim(&identity, expected_identity_id, master_key_hash) {
            info!(
                derived_id = %expected_id,
                "IdentityCreateFromOneTimeKey: recovered this claim's identity by its derived id \
                 (id and key bindings both verified)"
            );
            return Ok((identity.id(), identity));
        }
        // `recovered_identity_matches_claim` also fails closed when NO master
        // auth key hash was resolvable from the submitted keys (`master_key_hash
        // == None` — nothing was submitted, or `public_key_hash()` errored for
        // an unusual key type). The key binding can then never be established
        // for this claim, which is NOT evidence of a competing holder — report
        // the real cause. Terminal either way: the note is spent, and a retry
        // resubmits the same key set, so the hash stays unresolvable.
        if master_key_hash.is_none() {
            warn!(
                derived_id = %expected_id,
                "IdentityCreateFromOneTimeKey: an identity exists at this claim's derived id but \
                 this claim carries no resolvable master auth key hash, so ownership can be \
                 neither proven nor disproven"
            );
            return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
                reason: format!(
                    "identity {expected_id} was created from this invitation's notes, but this \
                     claim submitted no resolvable master authentication key hash, so its \
                     ownership cannot be verified: {evidence}"
                ),
            });
        }
        // The id matches (same nullifier set) but the on-chain keys are not ours:
        // another holder of the same bearer one-time key won the race.
        warn!(
            derived_id = %expected_id,
            "IdentityCreateFromOneTimeKey: an identity exists at this claim's derived id but does \
             not carry the submitted master auth key; another holder of the same one-time key \
             claimed the invitation first"
        );
        return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
            reason: format!(
                "identity {expected_id} was created from this invitation's notes but does not \
                 carry the submitted master authentication key, so it belongs to another holder \
                 of the one-time key: {evidence}"
            ),
        });
    }

    if spend_finalized {
        // Both probes came up empty under a definitive verdict + proven-spent
        // notes: the spend finalized without creating an identity that carries
        // this claim's bindings. That is the chargeable-`UnshieldAction`
        // fallback (the collision may have been on any submitted unique key,
        // not just MASTER) or a competing claim — terminal either way; the
        // value, if any, went to the creation-failure address.
        return Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
            reason: format!(
                "the claim's consensus verdict is definitive and the invitation notes are spent, \
                 but no identity carries this claim's bindings; the spend was finalized as a \
                 chargeable failure (or a competing claim) and created no identity for this \
                 wallet: {evidence}"
            ),
        });
    }

    Err(PlatformWalletError::ShieldedBroadcastUnconfirmed {
        identity_id: expected_id,
        reason: format!(
            "one-time-key claim executed (nullifier already spent) but the identity is not yet \
             resolvable by key hash or derived id: {evidence}"
        ),
    })
}

/// Classify a `wait_for_response` failure for an already-broadcast
/// shielded spend (see [`broadcast_shielded_spend_classified`]): unconfirmed, whatever
/// it is, the notes staying reserved.
///
/// A consensus verdict on the wait ([`carries_consensus_rejection`]) is no
/// proof either: the endpoint that sends it is unauthenticated, no proof or
/// quorum signature backs it, and one that kept or forwarded the signed
/// transition can answer with any verdict. DAPI also encodes its own
/// wait-side failures (timeouts, internal errors; see
/// `build_wait_for_state_transition_error_response` in rs-dapi) as
/// `StateTransitionBroadcastError`s with EMPTY consensus data. A spend
/// reported failed while it may still execute could be paid again with other
/// notes; one that never executes is released by the redrive and prune
/// passes once its anchor is pruned.
fn classify_spend_wait_failure(
    operation: &'static str,
    wait_err: &dash_sdk::Error,
) -> PlatformWalletError {
    warn!(
        operation,
        error = %wait_err,
        rejected = carries_consensus_rejection(wait_err),
        "Shielded spend broadcast but its result is unconfirmed; leaving the note reservations \
         in place"
    );
    PlatformWalletError::ShieldedSpendUnconfirmed {
        operation,
        reason: wait_err.to_string(),
    }
}

/// Helper to clone selection results out from under the store lock.
trait SelectionResultOwned {
    fn into_owned(self) -> (Vec<ShieldedNote>, u64, u64);
}

impl SelectionResultOwned for (Vec<&ShieldedNote>, u64, u64) {
    fn into_owned(self) -> (Vec<ShieldedNote>, u64, u64) {
        let (refs, total, fee) = self;
        let owned: Vec<ShieldedNote> = refs.into_iter().cloned().collect();
        (owned, total, fee)
    }
}

/// Convert a `PaymentAddress` to an `OrchardAddress` for the DPP builder.
fn payment_address_to_orchard(
    addr: &PaymentAddress,
) -> Result<OrchardAddress, PlatformWalletError> {
    let raw = addr.to_raw_address_bytes();
    OrchardAddress::from_raw_bytes(&raw).map_err(|_| {
        PlatformWalletError::ShieldedBuildError(
            "Failed to convert PaymentAddress to OrchardAddress".to_string(),
        )
    })
}

/// Deserialize an Orchard Note from 115 bytes.
///
/// Format: `recipient(43) || value(8 LE) || rho(32) || rseed(32)`.
/// Must be kept in sync with `serialize_note()` in sync.rs.
fn deserialize_note(data: &[u8]) -> Option<grovedb_commitment_tree::Note> {
    use grovedb_commitment_tree::{Note, NoteValue, RandomSeed, Rho};

    const SERIALIZED_NOTE_LEN: usize = 43 + 8 + 32 + 32;

    if data.len() != SERIALIZED_NOTE_LEN {
        return None;
    }

    let recipient_bytes: [u8; 43] = data[0..43].try_into().ok()?;
    let recipient = PaymentAddress::from_raw_address_bytes(&recipient_bytes).into_option()?;

    let value_bytes: [u8; 8] = data[43..51].try_into().ok()?;
    let value = NoteValue::from_raw(u64::from_le_bytes(value_bytes));

    let rho_bytes: [u8; 32] = data[51..83].try_into().ok()?;
    let rho = Rho::from_bytes(&rho_bytes).into_option()?;

    let rseed_bytes: [u8; 32] = data[83..115].try_into().ok()?;
    let rseed = RandomSeed::from_bytes(rseed_bytes, &rho).into_option()?;

    Note::from_parts(recipient, value, rho, rseed).into_option()
}

#[cfg(test)]
mod foreign_claim_guard_tests {
    use super::ForeignClaimGuards;
    use std::sync::Arc;

    /// Two callers with the same key must share ONE lifecycle mutex — that
    /// identity is what makes the claim single-flight (#4313 review finding
    /// 979bbc2fcb3c); different keys must not contend.
    #[test]
    fn same_key_shares_one_mutex_and_keys_are_independent() {
        let guards = ForeignClaimGuards::default();
        let a1 = guards.entry_for([1u8; 32]);
        let a2 = guards.entry_for([1u8; 32]);
        let b = guards.entry_for([2u8; 32]);
        assert!(
            Arc::ptr_eq(&a1, &a2),
            "same-key callers must receive the SAME lifecycle mutex"
        );
        assert!(
            !Arc::ptr_eq(&a1, &b),
            "distinct keys must receive distinct mutexes"
        );
    }

    /// The complete-lifecycle serialization: while one claim holds the
    /// guard (parked at an await, as across scan/proof/broadcast), a
    /// second same-key claim cannot enter; it proceeds only after the
    /// first releases — including release by CANCELLATION (future drop),
    /// so an abandoned claim can never wedge its invitation key.
    #[tokio::test]
    async fn same_key_claims_serialize_and_cancellation_releases() {
        let guards = Arc::new(ForeignClaimGuards::default());
        let key = [7u8; 32];

        let entry = guards.entry_for(key);
        let held = entry.lock().await;
        // Second same-key claim: must NOT be able to enter while held.
        let second = guards.entry_for(key);
        assert!(
            second.try_lock().is_err(),
            "a concurrent same-key claim must park while the lifecycle guard is held"
        );
        drop(held);
        assert!(
            second.try_lock().is_ok(),
            "the parked claim must proceed once the holder settles"
        );

        // Cancellation-safety: drop a future that acquired the guard at an
        // await point; the key must be immediately claimable again.
        let entry2 = guards.entry_for(key);
        let task = tokio::spawn(async move {
            let _g = entry2.lock().await;
            std::future::pending::<()>().await; // parked "mid-claim" forever
        });
        tokio::task::yield_now().await;
        task.abort();
        let _ = task.await;
        assert!(
            guards.entry_for(key).try_lock().is_ok(),
            "an aborted (cancelled) claim must release the key on drop"
        );
    }

    /// Abandoned keys cost nothing: once no claim holds a key's mutex, its
    /// registry row is pruned on the next acquisition, so hostile key churn
    /// cannot grow the map beyond the keys currently in flight.
    #[test]
    fn dead_entries_are_pruned() {
        let guards = ForeignClaimGuards::default();
        for i in 0..64u8 {
            let _ = guards.entry_for([i; 32]); // dropped immediately
        }
        let _live = guards.entry_for([0xFF; 32]);
        let len = guards
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        assert_eq!(
            len, 1,
            "only keys with a live claimant may occupy the registry"
        );
    }
}

#[cfg(test)]
mod shield_recipient_tests {
    use super::*;
    use crate::wallet::shielded::keys::OrchardKeySet;
    use dashcore::Network;

    fn keyset(seed_byte: u8) -> OrchardKeySet {
        OrchardKeySet::from_seed(&[seed_byte; 32], Network::Testnet, 0)
            .expect("ZIP-32 derivation from a fixed seed should succeed")
    }

    /// `None` = the self-shield: the default address, `Shield`/`In`,
    /// no counterparty — exactly what the pre-recipient path produced.
    #[test]
    fn no_recipient_resolves_to_the_default_address_as_shield_in() {
        let keys = keyset(0x42).viewing_keys();

        let resolved =
            resolve_shield_recipient(&keys, None).expect("self-shield must always resolve");

        assert_eq!(
            resolved.address.to_raw_bytes(),
            keys.default_address.to_raw_address_bytes(),
            "the self-shield note must go to the account's default address"
        );
        assert_eq!(resolved.counterparty, None);
        assert_eq!(resolved.kind, ShieldedActivityKind::Shield);
        assert_eq!(resolved.direction, ShieldedDirection::In);
    }

    /// A third-party address resolves to `Sent`/`Out` with the raw
    /// 43-byte address as counterparty — the classification the scan
    /// deriver produces for an OVK-recovered send to a non-own address,
    /// so live and restored rows agree.
    #[test]
    fn external_recipient_resolves_as_sent_out_with_raw_counterparty() {
        let keys = keyset(0x42).viewing_keys();
        let external = keyset(0x24).viewing_keys().default_address;

        let resolved = resolve_shield_recipient(&keys, Some(&external))
            .expect("a third-party recipient must resolve");

        assert_eq!(
            resolved.address.to_raw_bytes(),
            external.to_raw_address_bytes(),
            "the note must be built for the recipient's address"
        );
        assert_eq!(
            resolved.counterparty,
            Some(external.to_raw_address_bytes().to_vec()),
            "the activity row must carry the recipient as raw 43 bytes"
        );
        assert_eq!(resolved.kind, ShieldedActivityKind::Sent);
        assert_eq!(resolved.direction, ShieldedDirection::Out);
    }

    /// The account's own default address is not a third party: a
    /// `Sent`/`Out` live row for it would diverge from the self-pay row
    /// a restore's scan derives, so it must be rejected up front.
    #[test]
    fn own_default_address_as_recipient_is_rejected() {
        let keys = keyset(0x42).viewing_keys();
        let own = keys.default_address;

        let error = resolve_shield_recipient(&keys, Some(&own))
            .expect_err("the account's own address must be rejected");
        assert!(
            error
                .to_string()
                .contains("belongs to this shielded account"),
            "unexpected error: {error}"
        );
    }

    /// Orchard addresses are diversified, so ownership cannot be a
    /// fixed-address comparison: a non-default diversified index of the
    /// SAME account must also be recognized (via the IVK) and rejected.
    #[test]
    fn own_diversified_address_as_recipient_is_rejected() {
        let ks = keyset(0x42);
        let diversified = ks.address_at(7);
        let keys = ks.viewing_keys();
        assert_ne!(
            diversified.to_raw_address_bytes(),
            keys.default_address.to_raw_address_bytes(),
            "test needs a non-default diversified address"
        );

        resolve_shield_recipient(&keys, Some(&diversified))
            .expect_err("an own diversified address must be rejected");
    }
}

#[cfg(test)]
mod redrive_tests {
    use super::*;
    use crate::broadcast_outcome::test_sdks::{sdk_refusing_connections, sdk_without_addresses};
    use crate::wallet::identity::network::PaymentStarted;
    use crate::wallet::shielded::activity::ShieldedActivityEntry;
    use crate::wallet::shielded::store::InMemoryShieldedStore;
    use crate::wallet::shielded::FileBackedShieldedStore;
    use dash_sdk::dpp::dashcore::Network;
    use dash_sdk::dpp::prelude::CoreBlockHeight;
    use dash_sdk::error::ContextProviderError;
    use dash_sdk::error::StateTransitionBroadcastError;
    use dash_sdk::platform::ContextProvider;
    use dash_sdk::sdk::AddressList;
    use dpp::consensus::state::shielded::nullifier_already_spent_error::NullifierAlreadySpentError;
    use dpp::consensus::state::state_error::StateError;
    use dpp::consensus::ConsensusError;
    use dpp::data_contract::{DataContract, TokenConfiguration};
    use dpp::serialization::PlatformSerializable;
    use dpp::shielded::SerializedAction;
    use dpp::state_transition::private_document_create_from_shielded_pool_transition::methods::PrivateDocumentCreateFromShieldedPoolTransitionMethodsV0;
    use dpp::state_transition::private_document_create_from_shielded_pool_transition::PrivateDocumentCreateFromShieldedPoolTransition;
    use dpp::state_transition::shield_from_identity_transition::v0::ShieldFromIdentityTransitionV0;
    use dpp::state_transition::shield_transition::v0::ShieldTransitionV0;
    use dpp::state_transition::shielded_transfer_transition::v0::ShieldedTransferTransitionV0;
    use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::v0::IdentityCreateFromShieldedPoolTransitionV0;
    use dpp::version::PlatformVersion;
    use futures::{poll, FutureExt};
    use grovedb_commitment_tree::ProvingKey;
    use std::str::FromStr;
    use std::task::Poll;

    fn identity_shield_transition(identity_id: [u8; 32], nonce: u64) -> StateTransition {
        ShieldFromIdentityTransitionV0 {
            identity_id: identity_id.into(),
            amount: 1_000,
            actions: vec![],
            anchor: [1; 32],
            proof: vec![],
            binding_signature: [0; 64],
            nonce,
            user_fee_increase: 0,
            signature_public_key_id: 1,
            signature: BinaryData::new(vec![]),
        }
        .into()
    }

    #[tokio::test]
    async fn should_release_only_explicitly_abandoned_guard_and_never_redrive_it() {
        let wallet_id = [4; 32];
        let identity_id = [5; 32];
        let id = SubwalletId::new(wallet_id, 3);
        let mut store = InMemoryShieldedStore::new();
        let archived = PendingRedrive {
            activity_id: [6; 32],
            anchor: [1; 32],
            nullifiers: vec![],
            st_bytes: identity_shield_transition(identity_id, 7)
                .serialize_to_bytes()
                .unwrap(),
            attempts: 0,
            identity_nonce_finalized: false,
            identity_user_abandoned: true,
            anchor_height: None,
            identity_index: None,
        };
        store.arm_redrive(id, archived.clone()).unwrap();
        assert!(!has_unresolved_identity_shield(&store, wallet_id, identity_id).unwrap());
        let store = Arc::new(RwLock::new(store));
        // No mock nonce responses: accidentally retrying would reach an unconfigured SDK.
        redrive_pending_identity_shields(
            &Arc::new(dash_sdk::Sdk::new_mock()),
            &store,
            wallet_id,
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            store.read().await.pending_redrives(id).unwrap(),
            vec![archived.clone()]
        );
        let mut active = archived;
        active.activity_id = [7; 32];
        active.identity_user_abandoned = false;
        store
            .write()
            .await
            .arm_redrive(SubwalletId::new(wallet_id, 4), active)
            .unwrap();
        assert!(
            has_unresolved_identity_shield(&*store.read().await, wallet_id, identity_id).unwrap()
        );
    }

    #[tokio::test]
    async fn should_keep_delayed_identity_debit_guard_when_nonce_is_unused_then_finalized() {
        use dpp::identity::identity_nonce::validate_identity_nonce_update;
        let wallet_id = [4; 32];
        let identity_id = [5; 32];
        let id = SubwalletId::new(wallet_id, 3);
        let activity_id = [6; 32];
        let transition = identity_shield_transition(identity_id, 7);
        let signed_bytes = transition.serialize_to_bytes().unwrap();
        let mut store = InMemoryShieldedStore::new();
        store
            .arm_redrive(
                id,
                PendingRedrive {
                    activity_id,
                    anchor: [1; 32],
                    nullifiers: vec![],
                    st_bytes: signed_bytes.clone(),
                    attempts: 0,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
            )
            .unwrap();
        // A relay returned rejection while retaining the signed debit. Its
        // accompanying genuine nonce snapshot is still six. Seven can land later.
        assert!(!reconcile_identity_shield_nonce(&mut store, id, activity_id, Some(6), 7).unwrap());
        assert!(validate_identity_nonce_update(6, 7, identity_id.into()).is_valid());
        assert!(
            has_unresolved_identity_shield(&store, wallet_id, identity_id).unwrap(),
            "fresh-nonce replacement must stay blocked"
        );
        let pending = store.pending_redrives(id).unwrap();
        assert_eq!(pending[0].st_bytes, signed_bytes);
        assert!(
            !pending[0].identity_nonce_finalized,
            "original bytes remain eligible for safe retry"
        );
        // The original then executes. A new nonce would be valid too, which is
        // why nonce finality must not remove the unresolved-payment guard.
        assert!(validate_identity_nonce_update(7, 8, identity_id.into()).is_valid());
        assert!(reconcile_identity_shield_nonce(&mut store, id, activity_id, Some(7), 7).unwrap());
        assert!(has_unresolved_identity_shield(&store, wallet_id, identity_id).unwrap());
        assert!(store.pending_redrives(id).unwrap()[0].identity_nonce_finalized);
        let store = Arc::new(RwLock::new(store));
        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        redrive_pending_identity_shields(&sdk, &store, wallet_id, &CancellationToken::new()).await;
        assert_eq!(
            store.read().await.pending_redrives(id).unwrap()[0].attempts,
            0,
            "parked guard must perform no more broadcasts"
        );
        // The confirmation path can retire the guard, and nonce observation
        // racing behind it must not resurrect the resolved payment.
        store.write().await.clear_redrive(id, &activity_id).unwrap();
        reconcile_identity_shield_nonce(&mut *store.write().await, id, activity_id, Some(7), 7)
            .unwrap();
        assert!(
            !has_unresolved_identity_shield(&*store.read().await, wallet_id, identity_id).unwrap()
        );
    }

    #[tokio::test]
    async fn should_abort_before_broadcast_when_pending_activity_is_missing() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([4; 32], 0);
        let error =
            arm_identity_shield_redrive(&store, id, &None, &identity_shield_transition([5; 32], 1))
                .await
                .unwrap_err();
        assert!(matches!(error, PlatformWalletError::ShieldedBuildError(_)));
        assert!(store.read().await.pending_redrives(id).unwrap().is_empty());
    }

    #[test]
    fn unresolved_identity_debit_is_found_across_shielded_accounts() {
        let wallet_id = [4; 32];
        let identity_id = [5; 32];
        let transition = identity_shield_transition(identity_id, 7);
        let mut store = InMemoryShieldedStore::new();
        store
            .arm_redrive(
                SubwalletId::new(wallet_id, 3),
                PendingRedrive {
                    activity_id: [6; 32],
                    anchor: [1; 32],
                    nullifiers: vec![],
                    st_bytes: transition
                        .serialize_to_bytes()
                        .expect("identity shield should serialize"),
                    attempts: 0,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
            )
            .expect("redrive should arm");

        assert!(
            has_unresolved_identity_shield(&store, wallet_id, identity_id).expect("guard lookup")
        );
        assert!(
            !has_unresolved_identity_shield(&store, wallet_id, [9; 32]).expect("guard lookup"),
            "an unrelated identity must remain usable"
        );
    }

    #[test]
    fn corrupt_identity_debit_guard_fails_closed() {
        let wallet_id = [7; 32];
        let mut store = InMemoryShieldedStore::new();
        store
            .arm_redrive(
                SubwalletId::new(wallet_id, 0),
                PendingRedrive {
                    activity_id: [8; 32],
                    anchor: [1; 32],
                    nullifiers: vec![],
                    st_bytes: vec![0xde, 0xad],
                    attempts: 0,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
            )
            .expect("redrive should arm");

        assert!(
            has_unresolved_identity_shield(&store, wallet_id, [9; 32]).expect("guard lookup"),
            "an unreadable debit marker must not permit a fresh identity nonce"
        );
    }

    /// On a re-broadcast of our own byte-identical transition,
    /// `NullifierAlreadySpent` means the ORIGINAL executed — the
    /// classification must treat it as a success signal, distinct from
    /// every other consensus verdict.
    #[test]
    fn nullifier_already_spent_is_a_success_signal() {
        let cause = ConsensusError::StateError(StateError::NullifierAlreadySpentError(
            NullifierAlreadySpentError::new([1u8; 32]),
        ));
        let err = dash_sdk::Error::StateTransitionBroadcastError(StateTransitionBroadcastError {
            code: 1,
            message: "state error".to_string(),
            cause: Some(cause),
        });
        assert!(is_nullifier_already_spent(&err));
        // ...and it still counts as a consensus-rejection shape, so arm
        // ordering matters: the already-spent check must run first.
        assert!(carries_consensus_rejection(&err));

        let other = dash_sdk::Error::TimeoutReached(
            std::time::Duration::from_secs(1),
            "waiting".to_string(),
        );
        assert!(!is_nullifier_already_spent(&other));
    }

    /// The re-broadcast outcome classifier, arm order included:
    /// `NullifierAlreadySpent` is itself a consensus error, so the
    /// `AlreadyExecuted` arm must win over `DefinitiveRejection`.
    #[test]
    fn redrive_broadcast_classification_matrix() {
        assert_eq!(
            classify_redrive_broadcast(&Ok(())),
            RedriveBroadcastOutcome::Accepted
        );

        let already_spent = ConsensusError::StateError(StateError::NullifierAlreadySpentError(
            NullifierAlreadySpentError::new([1u8; 32]),
        ));
        let already_spent_err =
            dash_sdk::Error::StateTransitionBroadcastError(StateTransitionBroadcastError {
                code: 1,
                message: "state error".to_string(),
                cause: Some(already_spent),
            });
        assert_eq!(
            classify_redrive_broadcast(&Err(already_spent_err)),
            RedriveBroadcastOutcome::AlreadyExecuted,
            "already-spent must classify as success BEFORE the generic rejection arm"
        );

        let other_rejection = ConsensusError::BasicError(
            dpp::consensus::basic::BasicError::ProtocolVersionParsingError(
                dpp::consensus::basic::decode::ProtocolVersionParsingError::new(
                    "bad version".to_string(),
                ),
            ),
        );
        let rejection_err =
            dash_sdk::Error::StateTransitionBroadcastError(StateTransitionBroadcastError {
                code: 1,
                message: "state error".to_string(),
                cause: Some(other_rejection),
            });
        assert_eq!(
            classify_redrive_broadcast(&Err(rejection_err)),
            RedriveBroadcastOutcome::Inconclusive,
            "a refused re-broadcast does not prove the original never executes"
        );

        let timeout = dash_sdk::Error::TimeoutReached(
            std::time::Duration::from_secs(1),
            "waiting".to_string(),
        );
        assert_eq!(
            classify_redrive_broadcast(&Err(timeout)),
            RedriveBroadcastOutcome::Inconclusive
        );
    }

    #[tokio::test]
    async fn identity_redrive_ignores_spend_anchor_and_attempt_limit() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id = [0x45; 32];
        let previous_account = SubwalletId::new(wallet_id, 7);
        let activity_id = [0x67; 32];
        let transition = identity_shield_transition([0x89; 32], 11);
        store
            .write()
            .await
            .arm_redrive(
                previous_account,
                PendingRedrive {
                    activity_id,
                    anchor: [0xAB; 32],
                    nullifiers: vec![],
                    st_bytes: transition.serialize_to_bytes().expect("serialize"),
                    attempts: MAX_REDRIVE_ATTEMPTS,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
            )
            .expect("arm identity guard");

        redrive_pending_identity_shields(&sdk, &store, wallet_id, &CancellationToken::new()).await;

        let retained = store
            .read()
            .await
            .pending_redrives(previous_account)
            .expect("retained redrive");
        assert_eq!(
            retained.len(),
            1,
            "an endpoint error cannot clear the guard"
        );
        assert_eq!(
            retained[0].attempts,
            MAX_REDRIVE_ATTEMPTS + 1,
            "identity retries stay live beyond the note-spend attempt budget"
        );
    }

    /// The production payment flow saves a private contact request payment
    /// durably before its broadcast runs: intercepting the broadcast, a second
    /// handle on the store's file reads its journal record and redrive record,
    /// and the payment is marked started. A payment whose save fails is never
    /// broadcast, is not marked started, and releases its notes.
    #[tokio::test]
    async fn should_save_a_private_payment_durably_before_its_broadcast() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let action = SerializedAction {
            nullifier: [1; 32],
            rk: [2; 32],
            cmx: [3; 32],
            encrypted_note: vec![4; 216],
            cv_net: [5; 32],
            spend_auth_sig: [6; 64],
        };
        let payment = PrivateContactPayment {
            sender: [0xB1; 32],
            recipient: [0xA1; 32],
            account_reference: 5,
        };
        let wallet_id = [0x68; 32];
        let id = SubwalletId::new(wallet_id, 0);
        for save_fails in [false, true] {
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let store = Arc::new(RwLock::new(
                FileBackedShieldedStore::open_path(&path, 100).expect("the store"),
            ));
            let note = ShieldedNote {
                position: 0,
                cmx: [1; 32],
                nullifier: [0x5F; 32],
                block_height: 1,
                is_spent: false,
                value: 1_000_000_000_000,
                note_data: vec![],
            };
            store.write().await.save_note(id, &note).expect("a note");
            let (selected, _, _, reservation) = reserve_notes_covering(
                &sdk,
                &store,
                id,
                1,
                2,
                ShieldedFeeKind::Base,
                ChangeRequirement::Optional,
            )
            .await
            .expect("reserved");
            if save_fails {
                rusqlite::Connection::open(&path)
                    .expect("the store's file")
                    .execute_batch(
                        "CREATE TRIGGER reject_redrive_insert BEFORE INSERT ON \
                         shielded_pending_spends BEGIN SELECT RAISE(FAIL, 'injected'); END;",
                    )
                    .expect("the trigger");
            }
            let transition = PrivateDocumentCreateFromShieldedPoolTransition::try_from_bundle(
                Identifier::from([0x44; 32]),
                "privateContactRequest".to_string(),
                vec![BinaryData::new(vec![9; 589])],
                vec![action.clone(), action.clone()],
                0,
                [7; 32],
                vec![8; 64],
                [9; 64],
                PlatformVersion::latest(),
            )
            .expect("a type 29 transition");
            let started = PaymentStarted::default();
            let broadcast = AtomicBool::new(false);
            let submitted = submit_private_document_payment(
                PrivateDocumentSubmission {
                    store: &store,
                    persister: None,
                    wallet_id,
                    id,
                    pending_entry: Some(ShieldedActivityEntry {
                        id: [0x77; 32],
                        kind: ShieldedActivityKind::ShieldedSpend,
                        direction: ShieldedDirection::Out,
                        amount: 0,
                        fee: Some(0),
                        counterparty: None,
                        memo: None,
                        block_height: None,
                        status: ShieldedActivityStatus::Pending,
                        created_at_ms: 1,
                        min_note_position: None,
                        note_cmxs: vec![],
                        spent_nullifiers: vec![],
                    }),
                    selected_notes: &selected,
                    reservation,
                    state_transition: transition,
                    anchor: [7; 32],
                    anchor_height: 0,
                    private_contact: Some(payment),
                    journaled: Some(&[[9; 32]]),
                    payment_started: &started,
                },
                |_| {
                    broadcast.store(true, Ordering::SeqCst);
                    let durable =
                        FileBackedShieldedStore::open_path(&path, 100).expect("a second handle");
                    assert_eq!(
                        durable
                            .private_payment_journal(wallet_id)
                            .expect("the journal")
                            .len(),
                        1,
                        "the journal record is on the file"
                    );
                    assert_eq!(
                        durable
                            .private_contact_redrives_for_wallet(wallet_id)
                            .expect("the records")
                            .len(),
                        1,
                        "the redrive record and its association are on the file"
                    );
                    assert!(started.is_marked(), "marked once saved");
                    std::future::ready(Err(SpendFailure {
                        error: PlatformWalletError::ShieldedSpendUnconfirmed {
                            operation: "private document create from shielded pool",
                            reason: "intercepted".to_string(),
                        },
                        never_delivered: false,
                    }))
                },
            )
            .await;
            let unspent = store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .len();
            if save_fails {
                assert!(
                    matches!(submitted, Err(PlatformWalletError::ShieldedStoreError(_))),
                    "{submitted:?}"
                );
                assert!(!broadcast.load(Ordering::SeqCst), "never broadcast");
                assert!(!started.is_marked(), "never marked");
                assert_eq!(unspent, 1, "the notes are released");
                assert!(store
                    .read()
                    .await
                    .private_payment_journal(wallet_id)
                    .expect("the journal")
                    .is_empty());
            } else {
                assert!(
                    matches!(
                        submitted,
                        Err(PlatformWalletError::ShieldedSpendUnconfirmed { .. })
                    ),
                    "{submitted:?}"
                );
                assert!(broadcast.load(Ordering::SeqCst));
                assert_eq!(unspent, 0, "the notes stay reserved");
            }
        }
    }

    /// A payment dropped while its recovery save waits for the store has saved
    /// and broadcast nothing, and its note guard releases the notes once the
    /// store frees: the notes are handed over only after the save.
    #[tokio::test]
    async fn should_release_the_notes_of_a_payment_dropped_while_its_save_waits() {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};

        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let action = SerializedAction {
            nullifier: [1; 32],
            rk: [2; 32],
            cmx: [3; 32],
            encrypted_note: vec![4; 216],
            cv_net: [5; 32],
            spend_auth_sig: [6; 64],
        };
        let wallet_id = [0x69; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let note = ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5E; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        };
        store.write().await.save_note(id, &note).expect("a note");
        let (selected, _, _, reservation) = reserve_notes_covering(
            &sdk,
            &store,
            id,
            1,
            2,
            ShieldedFeeKind::Base,
            ChangeRequirement::Optional,
        )
        .await
        .expect("reserved");
        let transition = PrivateDocumentCreateFromShieldedPoolTransition::try_from_bundle(
            Identifier::from([0x44; 32]),
            "privateContactRequest".to_string(),
            vec![BinaryData::new(vec![9; 589])],
            vec![action.clone(), action],
            0,
            [7; 32],
            vec![8; 64],
            [9; 64],
            PlatformVersion::latest(),
        )
        .expect("a type 29 transition");
        let started = PaymentStarted::default();
        let mut cx = Context::from_waker(Waker::noop());

        let held = store.write().await;
        let mut submitted = Box::pin(submit_private_document_payment(
            PrivateDocumentSubmission {
                store: &store,
                persister: None,
                wallet_id,
                id,
                pending_entry: Some(ShieldedActivityEntry {
                    id: [0x78; 32],
                    kind: ShieldedActivityKind::ShieldedSpend,
                    direction: ShieldedDirection::Out,
                    amount: 0,
                    fee: Some(0),
                    counterparty: None,
                    memo: None,
                    block_height: None,
                    status: ShieldedActivityStatus::Pending,
                    created_at_ms: 1,
                    min_note_position: None,
                    note_cmxs: vec![],
                    spent_nullifiers: vec![],
                }),
                selected_notes: &selected,
                reservation,
                state_transition: transition,
                anchor: [7; 32],
                anchor_height: 0,
                private_contact: Some(PrivateContactPayment {
                    sender: [0xB1; 32],
                    recipient: [0xA1; 32],
                    account_reference: 5,
                }),
                journaled: Some(&[[9; 32]]),
                payment_started: &started,
            },
            |_| async { panic!("never broadcast") },
        ));
        // The submission waits for the store before its anchor step.
        assert!(submitted.as_mut().poll(&mut cx).is_pending());
        // Freed, the lock goes to the submission first; this request queues
        // behind it, so the submission's next one, the save, waits for it.
        drop(held);
        let mut ours = Box::pin(store.write());
        assert!(ours.as_mut().poll(&mut cx).is_pending());
        assert!(submitted.as_mut().poll(&mut cx).is_pending());
        let Poll::Ready(held) = ours.as_mut().poll(&mut cx) else {
            panic!("the store is ours once the submission's anchor step is done");
        };
        // Dropped while the save waits.
        drop(submitted);
        assert!(!started.is_marked(), "nothing was saved");
        drop(held);
        for _ in 0..50 {
            if !store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .len(),
            1,
            "the guard released the notes"
        );
        assert!(store
            .read()
            .await
            .private_payment_journal(wallet_id)
            .expect("the journal")
            .is_empty());
    }

    /// A stale spend attempt's detached cleanup, delayed until after an
    /// account purge and rebind (or the wallet's replacement) let a newer
    /// attempt reserve the same notes and save its payment, releases nothing:
    /// the newer attempt's notes stay reserved, and its redrive record,
    /// association and journal record stay.
    #[tokio::test]
    async fn should_not_release_a_newer_attempt_s_notes_from_a_stale_cleanup() {
        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let wallet_id = [0x6A; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let note = ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5D; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        };
        let payment = PrivateContactPayment {
            sender: [0xB1; 32],
            recipient: [0xA1; 32],
            account_reference: 5,
        };
        for wallet_replaced in [false, true] {
            let dir = tempfile::tempdir().expect("a directory");
            let store = Arc::new(RwLock::new(
                FileBackedShieldedStore::open_path(dir.path().join("tree.sqlite"), 100)
                    .expect("the store"),
            ));
            store.write().await.save_note(id, &note).expect("a note");
            let (_, _, _, stale) = reserve_notes_covering(
                &sdk,
                &store,
                id,
                1,
                2,
                ShieldedFeeKind::Base,
                ChangeRequirement::Optional,
            )
            .await
            .expect("reserved");
            // Dropped while the store is busy: its cleanup waits on a task.
            let mut held = store.write().await;
            drop(stale);
            if wallet_replaced {
                held.purge_wallet(wallet_id).expect("the wallet's purge");
            } else {
                held.purge_subwallet(id).expect("the account's purge");
            }
            held.save_note(id, &note).expect("the note again");
            assert!(held
                .mark_pending_by(id, &note.nullifier, u64::MAX)
                .expect("reserved by the newer attempt"));
            held.arm_private_contact_payment(
                id,
                PendingRedrive {
                    activity_id: [0x79; 32],
                    anchor: [7; 32],
                    nullifiers: vec![note.nullifier],
                    st_bytes: vec![0xAB; 96],
                    attempts: 0,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
                payment,
                &[PrivatePaymentEntry::for_commitment([0xC4; 32])],
                0,
                None,
            )
            .expect("the newer payment saved");
            drop(held);
            // The stale cleanup gets the store now.
            for _ in 0..10 {
                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            let store = store.read().await;
            let context = format!("wallet replaced: {wallet_replaced}");
            assert!(
                store.get_unspent_notes(id).expect("notes").is_empty(),
                "{context}: the newer attempt's note stays reserved"
            );
            assert_eq!(
                store.pending_redrives(id).expect("records").len(),
                1,
                "{context}"
            );
            assert_eq!(
                store
                    .private_contact_redrives_for_wallet(wallet_id)
                    .expect("associations")
                    .len(),
                1,
                "{context}"
            );
            assert_eq!(
                store
                    .private_payment_journal(wallet_id)
                    .expect("the journal")
                    .len(),
                1,
                "{context}"
            );
        }
    }

    /// A pending activity entry keyed `id`, as a spend records before it
    /// broadcasts.
    fn pending_entry(id: [u8; 32]) -> ShieldedActivityEntry {
        ShieldedActivityEntry {
            id,
            kind: ShieldedActivityKind::ShieldedSpend,
            direction: ShieldedDirection::Out,
            amount: 0,
            fee: Some(0),
            counterparty: None,
            memo: None,
            block_height: None,
            status: ShieldedActivityStatus::Pending,
            created_at_ms: 1,
            min_note_position: None,
            note_cmxs: vec![],
            spent_nullifiers: vec![],
        }
    }

    /// A private contact request payment's journal record outlives its notes'
    /// and redrive record whatever the spend's outcome, except a transition
    /// that provably never left the wallet. Settled unconfirmed (its
    /// broadcast may have been delivered and the result wait failed), the
    /// notes stay reserved and the record, association and journal record
    /// survive a reopen. Rejected after delivery, the notes and record are
    /// released, the journal record stays. Never delivered, the record and
    /// the journal record are retired together and the payment reads as not
    /// reaching the network; unless the redrive pass claimed a re-broadcast
    /// of the record meanwhile, when it may leave the wallet after all and is
    /// settled as unconfirmed, keeping everything.
    #[tokio::test]
    async fn should_keep_a_private_payment_s_journal_record_unless_it_never_left() {
        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let action = SerializedAction {
            nullifier: [1; 32],
            rk: [2; 32],
            cmx: [3; 32],
            encrypted_note: vec![4; 216],
            cv_net: [5; 32],
            spend_auth_sig: [6; 64],
        };
        let entry = BinaryData::new(vec![9; 589]);
        let commitment = [9u8; 32];
        let transition = PrivateDocumentCreateFromShieldedPoolTransition::try_from_bundle(
            Identifier::from([0x44; 32]),
            "privateContactRequest".to_string(),
            vec![entry],
            vec![action.clone(), action],
            0,
            [7; 32],
            vec![8; 64],
            [9; 64],
            PlatformVersion::latest(),
        )
        .expect("a type 29 transition");
        let payment = PrivateContactPayment {
            sender: [0xB1; 32],
            recipient: [0xA1; 32],
            account_reference: 5,
        };
        let wallet_id = [0x67; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let failure = |outcome: &str| match outcome {
            "unconfirmed" => SpendFailure {
                error: PlatformWalletError::ShieldedSpendUnconfirmed {
                    operation: "private document create from shielded pool",
                    reason: "the result wait timed out".to_string(),
                },
                never_delivered: false,
            },
            "rejected after delivery" => SpendFailure {
                error: PlatformWalletError::ShieldedBroadcastFailed(
                    "rejected on its merits".to_string(),
                ),
                never_delivered: false,
            },
            _ => SpendFailure {
                error: PlatformWalletError::ShieldedBroadcastFailed(
                    "refused at the door".to_string(),
                ),
                never_delivered: true,
            },
        };
        for outcome in [
            "unconfirmed",
            "rejected after delivery",
            "never delivered",
            "never delivered, re-broadcast claimed",
        ] {
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let store = Arc::new(RwLock::new(
                FileBackedShieldedStore::open_path(&path, 100).expect("the store"),
            ));
            let note = ShieldedNote {
                position: 0,
                cmx: [1; 32],
                nullifier: [0x5F; 32],
                block_height: 1,
                is_spent: false,
                value: 1_000_000_000_000,
                note_data: vec![],
            };
            store.write().await.save_note(id, &note).expect("a note");
            let (selected, _, _, reservation) = reserve_notes_covering(
                &sdk,
                &store,
                id,
                1,
                2,
                ShieldedFeeKind::Base,
                ChangeRequirement::Optional,
            )
            .await
            .expect("reserved");
            let started = PaymentStarted::default();
            arm_redrive_before_broadcast(
                &store,
                id,
                Some([0x77; 32]),
                [7; 32],
                selected.iter().map(|note| note.nullifier).collect(),
                &transition,
                "private document create from shielded pool",
                Some(payment),
                Some(&started),
                0,
            )
            .await
            .expect("saved before the broadcast");
            assert_eq!(
                store
                    .read()
                    .await
                    .private_payment_journal(wallet_id)
                    .expect("journal")
                    .len(),
                1,
                "{outcome}: journaled before the broadcast"
            );

            if outcome == "never delivered, re-broadcast claimed" {
                store
                    .write()
                    .await
                    .bump_redrive_attempts(id, &[0x77; 32])
                    .expect("claimed");
            }
            let settled = settle_private_document_payment(
                &store,
                None,
                wallet_id,
                id,
                &Some(pending_entry([0x77; 32])),
                reservation,
                &selected,
                Some(&[commitment]),
                &started,
                Err(failure(outcome)),
            )
            .await;
            assert!(settled.is_err(), "{outcome}: {settled:?}");
            let unspent = store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .len();
            drop(store);
            let reopened = FileBackedShieldedStore::open_path(&path, 100).expect("reopened");
            let journal = reopened
                .private_payment_journal(wallet_id)
                .expect("journal");
            let saved = reopened
                .private_contact_redrives_for_wallet(wallet_id)
                .expect("listed");
            match outcome {
                "unconfirmed" | "never delivered, re-broadcast claimed" => {
                    assert!(
                        matches!(
                            settled,
                            Err(PlatformWalletError::ShieldedSpendUnconfirmed { .. })
                        ),
                        "{outcome}: {settled:?}"
                    );
                    assert_eq!(unspent, 0, "the notes stay reserved");
                    assert_eq!(saved.len(), 1, "the record and its association stay");
                    assert_eq!(journal.len(), 1, "the journal record stays");
                    assert!(started.may_have_reached_network());
                }
                "rejected after delivery" => {
                    assert_eq!(unspent, 1, "the notes are released");
                    assert!(saved.is_empty(), "the record goes with them");
                    assert_eq!(journal.len(), 1, "the journal record stays");
                    assert_eq!(journal[0].commitment, commitment);
                    assert_eq!(journal[0].payment, payment);
                    assert!(started.may_have_reached_network());
                }
                _ => {
                    assert_eq!(unspent, 1, "the notes are released");
                    assert!(saved.is_empty(), "the record goes with them");
                    assert!(journal.is_empty(), "the journal record is discharged");
                    assert!(!started.may_have_reached_network());
                }
            }
        }
    }

    /// A spend that failed before anything was broadcast, here a private document payment
    /// whose anchor fetch the mock refuses, and is dropped while its rollback waits for the
    /// store lock another task holds, still releases its notes: the guard owns them until the
    /// rollback has cleared them.
    #[tokio::test]
    async fn should_release_the_notes_of_a_spend_dropped_during_its_failure_rollback() {
        struct UnreachableProver;
        impl OrchardProver for UnreachableProver {
            fn proving_key(&self) -> &ProvingKey {
                panic!("nothing is proved")
            }
        }

        let sdk = Arc::new(
            dash_sdk::SdkBuilder::new_mock()
                .with_version(PlatformVersion::latest())
                .build()
                .expect("a mock SDK"),
        );
        let wallet_id = [0x65; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let note = ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5E; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        };
        store.write().await.save_note(id, &note).expect("a note");
        let keys = OrchardKeySet::from_seed(&[0x42; 64], Network::Testnet, 0).expect("keys");
        let started = PaymentStarted::default();

        // The spend waits for the store behind this task; this task's next acquisition waits
        // behind the spend, so the lock passes to it right after the spend's reservation.
        let held = store.write().await;
        let mut spend = Box::pin(private_document_create_from_pool_with_metadata(
            &sdk,
            &store,
            None,
            wallet_id,
            &keys,
            0,
            Identifier::from([0x44; 32]),
            "privateContactRequest",
            vec![BinaryData::new(vec![1; 589])],
            None,
            &started,
            &UnreachableProver,
        ));
        assert!(poll!(&mut spend).is_pending(), "waits to reserve");
        let mut again = Box::pin(store.write());
        assert!(poll!(&mut again).is_pending(), "queued behind the spend");
        drop(held);
        // Reserved, refused by the anchor fetch, and waiting to roll back behind `again`.
        assert!(
            poll!(&mut spend).is_pending(),
            "the rollback waits for the store"
        );
        let Poll::Ready(contended) = poll!(&mut again) else {
            panic!("the lock passes on after the reservation");
        };
        assert!(
            contended.get_unspent_notes(id).expect("notes").is_empty(),
            "reserved while the rollback waits"
        );
        drop(spend);
        drop(contended);
        for _ in 0..100 {
            if !store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .iter()
                .map(|note| note.nullifier)
                .collect::<Vec<_>>(),
            vec![note.nullifier],
            "the note is spendable again"
        );
        assert!(!started.is_marked());
    }

    /// A reservation dropped while the store is locked is released once the
    /// lock frees, on a spawned task; one handed over stays.
    #[tokio::test]
    async fn should_release_a_dropped_reservation_once_the_store_frees() {
        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0x64; 32], 0);
        let note = ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5B; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        };
        store.write().await.save_note(id, &note).expect("a note");
        let unspent = || async {
            store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .len()
        };

        let (_, _, _, reservation) = reserve_notes_covering(
            &sdk,
            &store,
            id,
            1,
            2,
            ShieldedFeeKind::Base,
            ChangeRequirement::Optional,
        )
        .await
        .expect("reserved");
        assert_eq!(unspent().await, 0);
        let held = store.write().await;
        drop(reservation);
        drop(held);
        for _ in 0..100 {
            if unspent().await == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(unspent().await, 1, "released once the lock freed");

        let (_, _, _, mut kept) = reserve_notes_covering(
            &sdk,
            &store,
            id,
            1,
            2,
            ShieldedFeeKind::Base,
            ChangeRequirement::Optional,
        )
        .await
        .expect("reserved");
        kept.hand_over();
        drop(kept);
        assert_eq!(unspent().await, 0, "the spend owns it");
    }

    /// A spend dropped while it fetches the anchors, before its notes have an
    /// owner that frees them, leaves them spendable again: here a private
    /// document payment whose anchor fetch goes to a node that accepts the
    /// connection and never answers.
    #[tokio::test]
    async fn should_release_the_notes_of_a_spend_dropped_during_its_anchor_fetch() {
        struct NoKeys;
        impl ContextProvider for NoKeys {
            fn get_data_contract(
                &self,
                _id: &Identifier,
                _platform_version: &PlatformVersion,
            ) -> Result<Option<Arc<DataContract>>, ContextProviderError> {
                Ok(None)
            }
            fn get_token_configuration(
                &self,
                _token_id: &Identifier,
            ) -> Result<Option<TokenConfiguration>, ContextProviderError> {
                Ok(None)
            }
            fn get_quorum_public_key(
                &self,
                _quorum_type: u32,
                _quorum_hash: [u8; 32],
                _core_chain_locked_height: u32,
            ) -> Result<[u8; 48], ContextProviderError> {
                Err(ContextProviderError::Generic("no quorum keys".to_string()))
            }
            fn get_platform_activation_height(
                &self,
            ) -> Result<CoreBlockHeight, ContextProviderError> {
                Ok(1)
            }
        }
        struct UnreachableProver;
        impl OrchardProver for UnreachableProver {
            fn proving_key(&self) -> &ProvingKey {
                panic!("nothing is proved")
            }
        }

        /// A loopback node that accepts connections and never answers. Dropped,
        /// even while a failed assertion unwinds, it stops accepting, closes
        /// what it accepted and joins its thread.
        struct SilentNode {
            address: std::net::SocketAddr,
            shutdown: Arc<std::sync::atomic::AtomicBool>,
            thread: Option<std::thread::JoinHandle<()>>,
        }
        impl SilentNode {
            fn start() -> Self {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
                listener.set_nonblocking(true).expect("non-blocking accept");
                let address = listener.local_addr().expect("its address");
                let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let stop = Arc::clone(&shutdown);
                let thread = std::thread::spawn(move || {
                    let mut held = Vec::new();
                    while !stop.load(std::sync::atomic::Ordering::Acquire) {
                        match listener.accept() {
                            Ok((stream, _)) => held.push(stream),
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            Err(_) => break,
                        }
                    }
                    drop(held);
                });
                Self {
                    address,
                    shutdown,
                    thread: Some(thread),
                }
            }
        }
        impl Drop for SilentNode {
            fn drop(&mut self) {
                self.shutdown
                    .store(true, std::sync::atomic::Ordering::Release);
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }

        let node = SilentNode::start();
        let address = node.address;
        let sdk = Arc::new(
            dash_sdk::SdkBuilder::new(
                AddressList::from_str(&format!("http://{address}")).expect("an address"),
            )
            .with_network(Network::Testnet)
            .with_version(PlatformVersion::latest())
            .with_context_provider(NoKeys)
            .build()
            .expect("a network SDK"),
        );

        let wallet_id = [0x63; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let note = ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5A; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        };
        store.write().await.save_note(id, &note).expect("a note");
        let keys = OrchardKeySet::from_seed(&[0x42; 64], Network::Testnet, 0).expect("keys");
        let started = PaymentStarted::default();

        let paid = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            private_document_create_from_pool_with_metadata(
                &sdk,
                &store,
                None,
                wallet_id,
                &keys,
                0,
                Identifier::from([0x44; 32]),
                "privateContactRequest",
                vec![BinaryData::new(vec![1; 589])],
                None,
                &started,
                &UnreachableProver,
            ),
        )
        .await;
        assert!(paid.is_err(), "dropped during the anchor fetch: {paid:?}");
        assert!(!started.is_marked());
        let unspent = store.read().await.get_unspent_notes(id).expect("notes");
        assert_eq!(
            unspent
                .iter()
                .map(|note| note.nullifier)
                .collect::<Vec<_>>(),
            vec![note.nullifier],
            "the note is spendable again"
        );
    }

    /// A private payment is marked started only once its record is saved:
    /// dropped while the save waits for the store, the spend has saved
    /// nothing and is not marked, so its contact request's reservation is
    /// released rather than kept for a payment that never left. Once the
    /// lock frees, a save marks it.
    #[tokio::test]
    async fn should_mark_a_payment_started_only_once_its_record_is_saved() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0x62; 32], 0);
        let transition = identity_shield_transition([0xBB; 32], 4);
        let started = PaymentStarted::default();

        let held = store.write().await;
        let save = arm_redrive_before_broadcast(
            &store,
            id,
            Some([0x78; 32]),
            [7; 32],
            vec![[2; 32]],
            &transition,
            "test spend",
            None,
            Some(&started),
            0,
        );
        let mut save = Box::pin(save);
        assert!(
            (&mut save).now_or_never().is_none(),
            "the save waits for the store"
        );
        drop(save);
        drop(held);
        assert!(!started.is_marked(), "dropped before its record was saved");
        assert!(store
            .read()
            .await
            .pending_redrives(id)
            .expect("redrives")
            .is_empty());

        arm_redrive_before_broadcast(
            &store,
            id,
            Some([0x78; 32]),
            [7; 32],
            vec![[2; 32]],
            &transition,
            "test spend",
            None,
            Some(&started),
            0,
        )
        .await
        .expect("saved");
        assert!(started.is_marked());
        assert_eq!(
            store
                .read()
                .await
                .pending_redrives(id)
                .expect("redrives")
                .len(),
            1
        );
    }

    /// A spend's record is saved before its broadcast, keyed by its
    /// activity entry; one with no entry to key it by is refused, nothing
    /// saved.
    #[tokio::test]
    async fn should_save_a_spends_record_before_its_broadcast() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0x61; 32], 0);
        let transition = identity_shield_transition([0xBA; 32], 3);

        let refused = arm_redrive_before_broadcast(
            &store,
            id,
            None,
            [7; 32],
            vec![[1; 32]],
            &transition,
            "test spend",
            None,
            None,
            0,
        )
        .await;
        assert!(
            matches!(refused, Err(PlatformWalletError::ShieldedBuildError(_))),
            "{refused:?}"
        );
        assert!(store
            .read()
            .await
            .pending_redrives(id)
            .expect("redrives")
            .is_empty());

        arm_redrive_before_broadcast(
            &store,
            id,
            Some([0x77; 32]),
            [7; 32],
            vec![[1; 32]],
            &transition,
            "test spend",
            None,
            None,
            0,
        )
        .await
        .expect("saved");
        let saved = store.read().await.pending_redrives(id).expect("redrives");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].activity_id, [0x77; 32]);
        assert_eq!(saved[0].nullifiers, vec![[1; 32]]);
    }

    #[tokio::test]
    async fn ordinary_redrive_pass_leaves_identity_guard_untouched() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id = [0x54; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let anchor = [0x76; 32];
        let activity_id = [0x98; 32];
        store
            .write()
            .await
            .arm_redrive(
                id,
                PendingRedrive {
                    activity_id,
                    anchor,
                    nullifiers: vec![],
                    st_bytes: identity_shield_transition([0xBA; 32], 3)
                        .serialize_to_bytes()
                        .expect("serialize"),
                    attempts: 0,
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                    identity_index: None,
                },
            )
            .expect("arm identity guard");
        let recorded = [anchor].into_iter().collect();

        redrive_pending_spends(&sdk, &store, id, &recorded, &CancellationToken::new()).await;

        let retained = store.read().await.pending_redrives(id).expect("guard");
        assert_eq!(retained.len(), 1);
        assert_eq!(
            retained[0].attempts, 0,
            "the anchor-gated spend pass must not consume identity retry state"
        );
    }

    /// Decision paths that must NOT touch the network (the mock SDK has
    /// no broadcast expectation, so any attempt would error into the
    /// inconclusive arm and bump the counter): a pruned anchor belongs
    /// to the release backstop, an exhausted budget stays parked, and a
    /// corrupt stored transition is kept without a broadcast, as it may be
    /// the only evidence of a payment that can still execute.
    #[tokio::test]
    async fn should_skip_pruned_exhausted_and_undecodable_redrives_keeping_each() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id = [5u8; 32];
        let id = SubwalletId::new(wallet_id, 0);

        let rec = |activity: u8, anchor: u8, attempts: u32| PendingRedrive {
            activity_id: [activity; 32],
            anchor: [anchor; 32],
            nullifiers: vec![[activity ^ 0xFF; 32]],
            st_bytes: vec![0xDE, 0xAD], // never deserializes
            attempts,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
            identity_index: None,
        };
        {
            let mut guard = store.write().await;
            // Pruned anchor (10 not in recorded set) → untouched.
            guard.arm_redrive(id, rec(1, 10, 0)).unwrap();
            // Recorded anchor but attempts exhausted → untouched.
            guard
                .arm_redrive(id, rec(2, 11, MAX_REDRIVE_ATTEMPTS))
                .unwrap();
            // Recorded anchor, budget left, garbage bytes → kept, with no
            // broadcast.
            guard.arm_redrive(id, rec(3, 12, 0)).unwrap();
        }
        let recorded: std::collections::HashSet<[u8; 32]> =
            [[11u8; 32], [12u8; 32]].into_iter().collect();

        redrive_pending_spends(&sdk, &store, id, &recorded, &CancellationToken::new()).await;

        let left = store.read().await.pending_redrives(id).unwrap();
        let ids: Vec<[u8; 32]> = left.iter().map(|r| r.activity_id).collect();
        assert!(ids.contains(&[1u8; 32]), "pruned-anchor record left alone");
        assert!(ids.contains(&[2u8; 32]), "exhausted record left alone");
        assert!(
            ids.contains(&[3u8; 32]),
            "corrupt record kept without a broadcast attempt"
        );
        assert_eq!(
            left.iter().map(|r| r.attempts).max(),
            Some(MAX_REDRIVE_ATTEMPTS),
            "no attempt counters were bumped — nothing touched the network"
        );
    }

    /// A record retired while the redrive pass holds a snapshot that still
    /// has it is not re-broadcast: the pass claims each re-broadcast under
    /// the store write lock, from the record as it is then. Here the pass is
    /// held at its claim of an earlier record while the retirement queues for
    /// the lock, so it reaches the retired record with its stale snapshot.
    #[tokio::test]
    async fn should_not_rebroadcast_a_record_retired_after_the_pass_read_it() {
        use dash_sdk::platform::transition::broadcast_request::BroadcastRequestForStateTransition;
        use dpp::serialization::PlatformSerializable;
        use std::future::Future;
        use std::task::{Context, Waker};

        let mut sdk = dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk");
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0x5D; 32], 0);
        let anchor = [0x6D; 32];
        let earlier = identity_shield_transition([0xA1; 32], 3);
        let retired = identity_shield_transition([0xA2; 32], 4);
        let record = |activity: u8, transition: &StateTransition| PendingRedrive {
            activity_id: [activity; 32],
            anchor,
            nullifiers: vec![[activity ^ 0xFF; 32]],
            st_bytes: transition.serialize_to_bytes().expect("serializes"),
            attempts: 0,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
            identity_index: None,
        };
        {
            let mut guard = store.write().await;
            guard.arm_redrive(id, record(1, &earlier)).expect("armed");
            guard.arm_redrive(id, record(2, &retired)).expect("armed");
        }
        let recorded: std::collections::HashSet<[u8; 32]> = [anchor].into_iter().collect();
        let sdk_handle = Arc::new(sdk.clone());
        let cancel = CancellationToken::new();

        let mut cx = Context::from_waker(Waker::noop());
        let held = store.read().await;
        let mut pass = Box::pin(redrive_pending_spends(
            &sdk_handle,
            &store,
            id,
            &recorded,
            &cancel,
        ));
        assert!(
            pass.as_mut().poll(&mut cx).is_pending(),
            "the pass read both records and waits to claim the first"
        );
        let mut retire = Box::pin(async {
            store
                .write()
                .await
                .retire_undelivered_spend(id, &[2; 32], &[])
        });
        assert!(
            retire.as_mut().poll(&mut cx).is_pending(),
            "the retirement queues behind the claim"
        );
        drop(held);
        let ((), retirement) = tokio::join!(pass, retire);
        assert!(
            retirement.expect("retired"),
            "no re-broadcast was claimed for it"
        );

        let request = |transition: &StateTransition| {
            transition
                .broadcast_request_for_state_transition()
                .expect("serializes")
        };
        assert!(
            sdk.mock().request_count(&request(&earlier)).await > 0,
            "the earlier record is re-broadcast"
        );
        assert_eq!(
            sdk.mock().request_count(&request(&retired)).await,
            0,
            "the retired record is not"
        );
        let left = store.read().await.pending_redrives(id).expect("records");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].activity_id, [1; 32]);
        assert_eq!(left[0].attempts, 1, "claimed once, before its broadcast");
    }

    // ── Note-consuming spends: saved before the first dispatch ─────────

    /// The anchor the tests' spends are built on, and the proved height of
    /// the anchor set it was read from.
    const SPEND_ANCHOR: [u8; 32] = [0x6E; 32];
    const SPEND_ANCHOR_HEIGHT: u64 = 5;

    /// The identity the tests' identity create derives.
    const CREATED_IDENTITY: [u8; 32] = [0x33; 32];

    fn spend_action() -> SerializedAction {
        SerializedAction {
            nullifier: [1; 32],
            rk: [2; 32],
            cmx: [3; 32],
            encrypted_note: vec![4; 216],
            cv_net: [5; 32],
            spend_auth_sig: [6; 64],
        }
    }

    /// The note the tests' spends consume.
    fn spend_note() -> ShieldedNote {
        ShieldedNote {
            position: 0,
            cmx: [1; 32],
            nullifier: [0x5C; 32],
            block_height: 1,
            is_spent: false,
            value: 1_000_000_000_000,
            note_data: vec![],
        }
    }

    /// The note-consuming spends, by the code that saves, broadcasts and
    /// waits for them.
    #[derive(Clone, Copy, Debug)]
    enum SpendPath {
        /// Transfer, unshield, withdrawal and identity top-up
        /// ([`broadcast_shielded_spend_with_redrive_with_metadata`]): here a
        /// transfer.
        Shared,
        /// Identity create ([`broadcast_identity_create_with_redrive`]).
        IdentityCreate,
    }

    impl SpendPath {
        /// A signed transition of the path, as it saves and broadcasts one.
        fn transition(self) -> StateTransition {
            match self {
                Self::Shared => ShieldedTransferTransitionV0 {
                    actions: vec![spend_action(), spend_action()],
                    value_balance: 1_000,
                    anchor: SPEND_ANCHOR,
                    proof: vec![8; 64],
                    binding_signature: [9; 64],
                }
                .into(),
                Self::IdentityCreate => IdentityCreateFromShieldedPoolTransitionV0 {
                    public_keys: vec![],
                    denomination: 1_000_000,
                    actions: vec![spend_action(), spend_action()],
                    anchor: SPEND_ANCHOR,
                    proof: vec![8; 64],
                    binding_signature: [9; 64],
                    send_to_address_on_creation_failure: PlatformAddress::P2pkh([7; 20]),
                    identity_id: Identifier::from(CREATED_IDENTITY),
                }
                .into(),
            }
        }

        /// Save, broadcast and wait for the path's transition on `sdk`, as
        /// its production flow does once the transition is built.
        async fn broadcast(
            self,
            sdk: &Arc<dash_sdk::Sdk>,
            spend: &mut ReservedSpend,
            id: SubwalletId,
        ) -> Result<(), SpendFailure> {
            let transition = self.transition();
            match self {
                Self::Shared => broadcast_shielded_spend_with_redrive_with_metadata(
                    sdk,
                    &spend.store,
                    id,
                    &spend.entry,
                    SPEND_ANCHOR,
                    SPEND_ANCHOR_HEIGHT,
                    &spend.notes,
                    &mut spend.reservation,
                    &transition,
                    "transfer",
                    SpendResultWait::ExecutionProved,
                )
                .await
                .map(|_| ()),
                Self::IdentityCreate => broadcast_identity_create_with_redrive(
                    sdk,
                    &spend.store,
                    id,
                    &spend.entry,
                    SPEND_ANCHOR,
                    SPEND_ANCHOR_HEIGHT,
                    &spend.notes,
                    &mut spend.reservation,
                    &transition,
                    Identifier::from(CREATED_IDENTITY),
                    BTreeMap::new(),
                )
                .await
                .map(|_| ()),
            }
        }

        /// Settle the path's failed spend, as its production flow does, and
        /// return the error it reports.
        async fn settle(
            self,
            spend: ReservedSpend,
            wallet_id: WalletId,
            id: SubwalletId,
            failure: SpendFailure,
        ) -> PlatformWalletError {
            let ReservedSpend {
                store,
                reservation,
                entry,
                ..
            } = spend;
            match self {
                Self::Shared => {
                    settle_failed_spend(
                        &store,
                        None,
                        wallet_id,
                        id,
                        &entry,
                        reservation,
                        "transfer",
                        failure,
                    )
                    .await
                }
                Self::IdentityCreate => identity_create_failure(
                    settle_failed_spend(
                        &store,
                        None,
                        wallet_id,
                        id,
                        &entry,
                        reservation,
                        IDENTITY_CREATE_OPERATION,
                        failure,
                    )
                    .await,
                    Some(Identifier::from(CREATED_IDENTITY)),
                ),
            }
        }

        /// Whether `error` is the path's report of a spend whose outcome is
        /// not known.
        fn reads_unconfirmed(self, error: &PlatformWalletError) -> bool {
            match self {
                Self::Shared => matches!(
                    error,
                    PlatformWalletError::ShieldedSpendUnconfirmed {
                        operation: "transfer",
                        ..
                    }
                ),
                Self::IdentityCreate => matches!(
                    error,
                    PlatformWalletError::ShieldedBroadcastUnconfirmed { identity_id, .. }
                        if *identity_id == Identifier::from(CREATED_IDENTITY)
                ),
            }
        }
    }

    /// A spend right before it saves its record: its note reserved in a
    /// file-backed store, and its pending activity entry recorded.
    struct ReservedSpend {
        store: Arc<RwLock<FileBackedShieldedStore>>,
        notes: Vec<ShieldedNote>,
        reservation: NoteReservation<FileBackedShieldedStore>,
        entry: Option<ShieldedActivityEntry>,
    }

    impl ReservedSpend {
        async fn at(path: &std::path::Path, id: SubwalletId, activity_id: [u8; 32]) -> Self {
            let sdk = Arc::new(dash_sdk::Sdk::new_mock());
            let store = Arc::new(RwLock::new(
                FileBackedShieldedStore::open_path(path, 100).expect("the store"),
            ));
            store
                .write()
                .await
                .save_note(id, &spend_note())
                .expect("a note");
            let (notes, _, _, reservation) = reserve_notes_covering(
                &sdk,
                &store,
                id,
                1,
                2,
                ShieldedFeeKind::Base,
                ChangeRequirement::Optional,
            )
            .await
            .expect("reserved");
            let entry = pending_entry(activity_id);
            store
                .write()
                .await
                .save_activity(id, &entry)
                .expect("the pending row");
            Self {
                store,
                notes,
                reservation,
                entry: Some(entry),
            }
        }
    }

    /// Make the store at `path` refuse to save a spend's record.
    fn refuse_record_saves(path: &std::path::Path) {
        rusqlite::Connection::open(path)
            .expect("the store's file")
            .execute_batch(
                "CREATE TRIGGER reject_redrive_insert BEFORE INSERT ON \
                 shielded_pending_spends BEGIN SELECT RAISE(FAIL, 'injected'); END;",
            )
            .expect("the trigger");
    }

    /// The store at `path` as a restart opens it, the wallet's note loaded
    /// into it again.
    fn restarted_store(path: &std::path::Path, id: SubwalletId) -> FileBackedShieldedStore {
        let mut store = FileBackedShieldedStore::open_path(path, 100).expect("reopened");
        store
            .save_note(id, &spend_note())
            .expect("the note, loaded again");
        store
    }

    /// How many notes of `id` a spend can select, and the status of the
    /// activity row `activity_id`.
    async fn spendable_and_status(
        store: &Arc<RwLock<FileBackedShieldedStore>>,
        id: SubwalletId,
        activity_id: &[u8; 32],
    ) -> (usize, ShieldedActivityStatus) {
        let store = store.read().await;
        (
            store.get_unspent_notes(id).expect("notes").len(),
            store
                .get_activity_by_entry_id(id, activity_id)
                .expect("the row reads")
                .expect("the row")
                .status,
        )
    }

    /// A note-consuming spend saves its record durably before its first
    /// dispatch, and hands its notes over only then. So a process that exits
    /// between the broadcast and the result (here the spend is dropped while
    /// its result wait is pending, and nothing settles it) leaves the record:
    /// the store a restart opens holds the notes' reservation and the exact
    /// signed transition, the redrive pass re-broadcasts it, and the scan
    /// that sees its nullifier spent resolves it.
    #[tokio::test]
    async fn should_restore_a_spend_that_exited_between_its_broadcast_and_its_result() {
        use dash_sdk::platform::transition::broadcast_request::BroadcastRequestForStateTransition;
        use std::task::{Context, Waker};

        let mut sdk = dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk");
        let sdk_handle = Arc::new(sdk.clone());
        let id = SubwalletId::new([0x71; 32], 0);
        let activity_id = [0x7A; 32];
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("tree.sqlite");
        let ReservedSpend {
            store,
            notes,
            mut reservation,
            entry,
        } = ReservedSpend::at(&path, id, activity_id).await;
        let transition = SpendPath::Shared.transition();
        let signed = transition.serialize_to_bytes().expect("serializes");

        let mut cx = Context::from_waker(Waker::noop());
        let mut spend = Box::pin(save_spend_then_broadcast(
            &store,
            id,
            &entry,
            SPEND_ANCHOR,
            SPEND_ANCHOR_HEIGHT,
            &notes,
            &mut reservation,
            &transition,
            "transfer",
            || {
                // Nothing is dispatched before the record is on the file.
                let durable =
                    FileBackedShieldedStore::open_path(&path, 100).expect("a second handle");
                let saved = durable.pending_redrives(id).expect("the records");
                assert_eq!(saved.len(), 1, "saved before the first dispatch");
                assert_eq!(saved[0].st_bytes, signed);
                // Dispatched; its result never comes.
                std::future::pending::<Result<(), SpendFailure>>()
            },
        ));
        assert!(
            spend.as_mut().poll(&mut cx).is_pending(),
            "broadcast, waiting for its result"
        );
        // The process exits here: the spend never returns, nothing settles it.
        drop(spend);
        drop(reservation);
        assert!(
            store
                .read()
                .await
                .get_unspent_notes(id)
                .expect("notes")
                .is_empty(),
            "the dropped spend's notes stay reserved"
        );
        drop(store);

        let restarted = Arc::new(RwLock::new(restarted_store(&path, id)));
        {
            let store = restarted.read().await;
            assert!(
                store.get_unspent_notes(id).expect("notes").is_empty(),
                "the reservation is restored from the record"
            );
            let saved = store.pending_redrives(id).expect("the records");
            assert_eq!(saved.len(), 1);
            assert_eq!(saved[0].st_bytes, signed, "the exact signed transition");
            assert_eq!(saved[0].nullifiers, vec![spend_note().nullifier]);
            assert_eq!(saved[0].anchor, SPEND_ANCHOR);
            assert_eq!(saved[0].anchor_height, Some(SPEND_ANCHOR_HEIGHT));
            assert_eq!(saved[0].attempts, 0);
        }

        let recorded = [SPEND_ANCHOR].into_iter().collect();
        redrive_pending_spends(
            &sdk_handle,
            &restarted,
            id,
            &recorded,
            &CancellationToken::new(),
        )
        .await;
        let request = transition
            .broadcast_request_for_state_transition()
            .expect("serializes");
        assert!(
            sdk.mock().request_count(&request).await > 0,
            "the redrive pass re-broadcasts the saved transition"
        );
        assert_eq!(
            restarted
                .read()
                .await
                .pending_redrives(id)
                .expect("records")[0]
                .attempts,
            1
        );

        // The next scan sees the spend's nullifier on chain.
        assert!(restarted
            .write()
            .await
            .mark_spent(id, &spend_note().nullifier)
            .expect("marked"));
        drop(restarted);
        assert!(
            FileBackedShieldedStore::open_path(&path, 100)
                .expect("reopened")
                .pending_redrives(id)
                .expect("records")
                .is_empty(),
            "resolved for good"
        );
    }

    /// A failed spend is settled by what is known about its transition. One
    /// that never left the wallet has its record retired and its notes
    /// released together, unless a redrive pass claimed a re-broadcast of the
    /// record meanwhile: it may then leave the wallet after all, and is kept
    /// as unconfirmed. One whose record was saved and that may have reached a
    /// node keeps its notes and its record whatever error ended it, and reads
    /// as unconfirmed. One whose record could not be saved was never
    /// dispatched, and releases its notes.
    #[tokio::test]
    async fn should_release_a_failed_spend_s_notes_only_when_its_transition_cannot_execute() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let wallet_id = [0x72; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x7B; 32];
        let transition = SpendPath::Shared.transition();
        let never_delivered = || SpendFailure {
            error: PlatformWalletError::ShieldedBroadcastFailed("no address".to_string()),
            never_delivered: true,
        };
        for outcome in [
            "never delivered",
            "never delivered, re-broadcast claimed",
            "unconfirmed",
            "failed after its record was saved",
            "its record could not be saved",
        ] {
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let mut spend = ReservedSpend::at(&path, id, activity_id).await;
            if outcome == "its record could not be saved" {
                refuse_record_saves(&path);
            }
            let dispatched = AtomicBool::new(false);
            let store = Arc::clone(&spend.store);
            let result: Result<(), SpendFailure> = save_spend_then_broadcast(
                &spend.store,
                id,
                &spend.entry,
                SPEND_ANCHOR,
                SPEND_ANCHOR_HEIGHT,
                &spend.notes,
                &mut spend.reservation,
                &transition,
                "transfer",
                || async {
                    dispatched.store(true, Ordering::SeqCst);
                    Err(match outcome {
                        "never delivered" => never_delivered(),
                        "never delivered, re-broadcast claimed" => {
                            // A redrive pass re-broadcast the saved record
                            // while the spend's own broadcast ran.
                            store
                                .write()
                                .await
                                .bump_redrive_attempts(id, &activity_id)
                                .expect("claimed");
                            never_delivered()
                        }
                        "unconfirmed" => PlatformWalletError::ShieldedSpendUnconfirmed {
                            operation: "transfer",
                            reason: "the result wait timed out".to_string(),
                        }
                        .into(),
                        _ => PlatformWalletError::ShieldedBuildError(
                            "its proved result did not read".to_string(),
                        )
                        .into(),
                    })
                },
            )
            .await;
            let failure = result.expect_err("the spend failed");
            let store = Arc::clone(&spend.store);
            let settled = SpendPath::Shared
                .settle(spend, wallet_id, id, failure)
                .await;
            let (spendable, status) = spendable_and_status(&store, id, &activity_id).await;
            drop(store);
            let saved = FileBackedShieldedStore::open_path(&path, 100)
                .expect("reopened")
                .pending_redrives(id)
                .expect("records");
            match outcome {
                "never delivered" => {
                    assert!(
                        matches!(settled, PlatformWalletError::ShieldedBroadcastFailed(_)),
                        "{outcome}: {settled:?}"
                    );
                    assert_eq!(spendable, 1, "{outcome}: the notes are released");
                    assert!(saved.is_empty(), "{outcome}: the record is retired");
                    assert_eq!(status, ShieldedActivityStatus::Failed, "{outcome}");
                }
                "its record could not be saved" => {
                    assert!(
                        matches!(settled, PlatformWalletError::ShieldedStoreError(_)),
                        "{outcome}: {settled:?}"
                    );
                    assert!(!dispatched.load(Ordering::SeqCst), "never dispatched");
                    assert_eq!(spendable, 1, "{outcome}: the notes are released");
                    assert!(saved.is_empty(), "{outcome}");
                    assert_eq!(status, ShieldedActivityStatus::Failed, "{outcome}");
                }
                _ => {
                    assert!(
                        SpendPath::Shared.reads_unconfirmed(&settled),
                        "{outcome}: {settled:?}"
                    );
                    assert_eq!(spendable, 0, "{outcome}: the notes stay reserved");
                    assert_eq!(saved.len(), 1, "{outcome}: the record stays");
                    assert_eq!(status, ShieldedActivityStatus::Pending, "{outcome}");
                }
            }
        }
    }

    /// Each note-consuming spend path, on an SDK whose node answers every
    /// request with an error: a dispatch may have reached the node, so the
    /// spend is unconfirmed, never failed. Its notes stay reserved and its
    /// record stays, across a restart too, where the redrive pass
    /// re-broadcasts the exact signed transition and the scan that sees its
    /// nullifier spent resolves it.
    #[tokio::test(start_paused = true)]
    async fn should_keep_a_spend_that_may_have_reached_a_node_until_it_resolves() {
        use dash_sdk::platform::transition::broadcast_request::BroadcastRequestForStateTransition;

        let wallet_id = [0x73; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x7C; 32];
        for spend_path in [SpendPath::Shared, SpendPath::IdentityCreate] {
            let mut sdk = dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk");
            let sdk_handle = Arc::new(sdk.clone());
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let mut spend = ReservedSpend::at(&path, id, activity_id).await;
            let transition = spend_path.transition();
            let request = transition
                .broadcast_request_for_state_transition()
                .expect("serializes");

            let failure = spend_path
                .broadcast(&sdk_handle, &mut spend, id)
                .await
                .expect_err("its result is not proved");
            assert!(!failure.never_delivered, "{spend_path:?}");
            let dispatches = sdk.mock().request_count(&request).await;
            assert!(dispatches > 0, "{spend_path:?}: it was dispatched");
            let store = Arc::clone(&spend.store);
            let settled = spend_path.settle(spend, wallet_id, id, failure).await;
            assert!(
                spend_path.reads_unconfirmed(&settled),
                "{spend_path:?}: {settled:?}"
            );
            assert_eq!(
                spendable_and_status(&store, id, &activity_id).await,
                (0, ShieldedActivityStatus::Pending),
                "{spend_path:?}: the notes stay reserved, the row pending"
            );
            drop(store);

            let restarted = Arc::new(RwLock::new(restarted_store(&path, id)));
            {
                let store = restarted.read().await;
                assert!(
                    store.get_unspent_notes(id).expect("notes").is_empty(),
                    "{spend_path:?}: the reservation is restored from the record"
                );
                let saved = store.pending_redrives(id).expect("the records");
                assert_eq!(saved.len(), 1, "{spend_path:?}");
                assert_eq!(
                    saved[0].st_bytes,
                    transition.serialize_to_bytes().expect("serializes"),
                    "{spend_path:?}: the exact signed transition"
                );
            }
            let recorded = [SPEND_ANCHOR].into_iter().collect();
            redrive_pending_spends(
                &sdk_handle,
                &restarted,
                id,
                &recorded,
                &CancellationToken::new(),
            )
            .await;
            assert!(
                sdk.mock().request_count(&request).await > dispatches,
                "{spend_path:?}: the redrive pass re-broadcasts it"
            );
            assert!(restarted
                .write()
                .await
                .mark_spent(id, &spend_note().nullifier)
                .expect("marked"));
            assert!(
                restarted
                    .read()
                    .await
                    .pending_redrives(id)
                    .expect("records")
                    .is_empty(),
                "{spend_path:?}: resolved once its nullifier is seen spent"
            );
        }
    }

    /// Each note-consuming spend path, on an SDK with no address to send to:
    /// the transition never left the wallet. Its record was saved before the
    /// dispatch that was refused; the settlement retires it and frees the
    /// notes, and the spend reads as failed.
    #[tokio::test]
    async fn should_retire_the_record_of_a_spend_that_never_left_the_wallet() {
        let wallet_id = [0x74; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x7D; 32];
        for spend_path in [SpendPath::Shared, SpendPath::IdentityCreate] {
            let sdk = Arc::new(sdk_without_addresses());
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let mut spend = ReservedSpend::at(&path, id, activity_id).await;

            let failure = spend_path
                .broadcast(&sdk, &mut spend, id)
                .await
                .expect_err("nothing to send it to");
            assert!(
                failure.never_delivered,
                "{spend_path:?}: {:?}",
                failure.error
            );
            let store = Arc::clone(&spend.store);
            assert_eq!(
                store
                    .read()
                    .await
                    .pending_redrives(id)
                    .expect("records")
                    .len(),
                1,
                "{spend_path:?}: its record was saved before its dispatch"
            );
            let settled = spend_path.settle(spend, wallet_id, id, failure).await;
            assert!(
                matches!(settled, PlatformWalletError::ShieldedBroadcastFailed(_)),
                "{spend_path:?}: {settled:?}"
            );
            assert_eq!(
                spendable_and_status(&store, id, &activity_id).await,
                (1, ShieldedActivityStatus::Failed),
                "{spend_path:?}: the notes are free, the row failed"
            );
            drop(store);
            assert!(
                FileBackedShieldedStore::open_path(&path, 100)
                    .expect("reopened")
                    .pending_redrives(id)
                    .expect("records")
                    .is_empty(),
                "{spend_path:?}: no record is left"
            );
        }
    }

    /// Each note-consuming spend path whose record cannot be saved dispatches
    /// nothing: the spend is refused, and its notes are released.
    #[tokio::test]
    async fn should_not_dispatch_a_spend_whose_record_cannot_be_saved() {
        use dash_sdk::platform::transition::broadcast_request::BroadcastRequestForStateTransition;

        let wallet_id = [0x75; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x7E; 32];
        for spend_path in [SpendPath::Shared, SpendPath::IdentityCreate] {
            let mut sdk = dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk");
            let sdk_handle = Arc::new(sdk.clone());
            let dir = tempfile::tempdir().expect("a directory");
            let path = dir.path().join("tree.sqlite");
            let mut spend = ReservedSpend::at(&path, id, activity_id).await;
            refuse_record_saves(&path);

            let failure = spend_path
                .broadcast(&sdk_handle, &mut spend, id)
                .await
                .expect_err("its record is not saved");
            assert!(
                matches!(failure.error, PlatformWalletError::ShieldedStoreError(_)),
                "{spend_path:?}: {:?}",
                failure.error
            );
            let request = spend_path
                .transition()
                .broadcast_request_for_state_transition()
                .expect("serializes");
            assert_eq!(
                sdk.mock().request_count(&request).await,
                0,
                "{spend_path:?}: nothing was dispatched"
            );
            let store = Arc::clone(&spend.store);
            let settled = spend_path.settle(spend, wallet_id, id, failure).await;
            assert!(
                matches!(settled, PlatformWalletError::ShieldedStoreError(_)),
                "{spend_path:?}: {settled:?}"
            );
            assert_eq!(
                spendable_and_status(&store, id, &activity_id).await,
                (1, ShieldedActivityStatus::Failed),
                "{spend_path:?}: the notes are free, the row failed"
            );
        }
    }

    // ── Shields from Platform addresses ────────────────────────────────

    /// A signed shield from a Platform address (type 15).
    fn shield_transition() -> StateTransition {
        ShieldTransitionV0 {
            inputs: BTreeMap::from([(SHIELD_INPUT, (SHIELD_NONCE, 2_000_000))]),
            actions: vec![spend_action(), spend_action()],
            amount: 1_000_000,
            anchor: SPEND_ANCHOR,
            proof: vec![8; 64],
            binding_signature: [9; 64],
            fee_strategy: vec![AddressFundsFeeStrategyStep::DeductFromInput(0)],
            user_fee_increase: 0,
            input_witnesses: vec![],
        }
        .into()
    }

    /// A node's refusal of a dispatch, as an offline or overloaded endpoint
    /// answers one.
    fn unavailable() -> dash_sdk::Error {
        use dash_sdk::dapi_client::transport::TransportError;
        use dash_sdk::dapi_client::DapiClientError;
        use dash_sdk::dapi_grpc::tonic::{Code, Status};

        dash_sdk::Error::DapiClientError(DapiClientError::Transport(TransportError::Grpc(
            Status::new(Code::Unavailable, "test"),
        )))
    }

    /// A stale-nonce verdict on the shield's input, as a broadcast or as the
    /// result wait returns one.
    fn nonce_verdict(on_the_wait: bool) -> dash_sdk::Error {
        use dpp::consensus::state::address_funds::AddressInvalidNonceError;

        let verdict = ConsensusError::StateError(StateError::AddressInvalidNonceError(
            AddressInvalidNonceError::new(PlatformAddress::P2pkh([7; 20]), 1, 2),
        ));
        if on_the_wait {
            dash_sdk::Error::StateTransitionBroadcastError(StateTransitionBroadcastError {
                code: 1,
                message: "rejected".to_string(),
                cause: Some(verdict),
            })
        } else {
            dash_sdk::Error::Protocol(dpp::ProtocolError::ConsensusError(Box::new(verdict)))
        }
    }

    fn wait_timeout() -> dash_sdk::Error {
        dash_sdk::Error::TimeoutReached(std::time::Duration::from_secs(80), "waiting".to_string())
    }

    /// The input address of [`shield_transition`] and the nonce it signed.
    const SHIELD_INPUT: PlatformAddress = PlatformAddress::P2pkh([7; 20]);
    const SHIELD_NONCE: AddressNonce = 1;

    /// What [`shield_transition`] signed: its input addresses and nonces.
    fn shield_inputs() -> BTreeMap<PlatformAddress, (AddressNonce, Credits)> {
        shield_transition()
            .inputs()
            .expect("a shield spends from addresses")
            .clone()
    }

    /// A verified result reading the shield's input address at `nonce`, or
    /// with no state when `None`.
    fn input_snapshot(nonce: Option<AddressNonce>) -> StateTransitionProofResult {
        StateTransitionProofResult::VerifiedAddressInfos(BTreeMap::from([(
            SHIELD_INPUT,
            nonce.map(|nonce| (nonce, 1_000_000)),
        )]))
    }

    /// An in-memory store holding the pending activity row `activity_id` of a
    /// shield of `id`, and the row's entry.
    async fn recorded_shield(
        id: SubwalletId,
        activity_id: [u8; 32],
    ) -> (
        Arc<RwLock<InMemoryShieldedStore>>,
        Option<ShieldedActivityEntry>,
    ) {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let entry = pending_entry(activity_id);
        store
            .write()
            .await
            .save_activity(id, &entry)
            .expect("the pending row");
        (store, Some(entry))
    }

    async fn shield_status(
        store: &Arc<RwLock<InMemoryShieldedStore>>,
        id: SubwalletId,
        activity_id: &[u8; 32],
    ) -> ShieldedActivityStatus {
        store
            .read()
            .await
            .get_activity_by_entry_id(id, activity_id)
            .expect("the row reads")
            .expect("the row")
            .status
    }

    /// Once a dispatch may have reached a node, a shield whose result is not
    /// proved is unconfirmed and its row stays pending, never failed: a node's
    /// refusal of the broadcast and a consensus verdict on either call
    /// included (here a stale nonce, which used to read as a retryable nonce
    /// mismatch), as no proof backs an endpoint's answer and a retry signs a
    /// second shield. The reason keeps what the broadcast was answered with.
    #[tokio::test]
    async fn should_report_a_shield_unconfirmed_once_a_dispatch_may_have_reached_a_node() {
        let wallet_id = [0x76; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x7F; 32];
        let cases = [
            (
                "a node refused the broadcast, the wait timed out",
                BroadcastDelivery::MaybeDelivered(unavailable()),
                wait_timeout(),
            ),
            (
                "the broadcast was answered with a consensus verdict",
                BroadcastDelivery::MaybeDelivered(nonce_verdict(false)),
                wait_timeout(),
            ),
            (
                "admitted, the wait was answered with a consensus verdict",
                BroadcastDelivery::Admitted,
                nonce_verdict(true),
            ),
            (
                "admitted, the wait timed out",
                BroadcastDelivery::Admitted,
                wait_timeout(),
            ),
        ];
        for (case, delivery, wait_error) in cases {
            let (store, entry) = recorded_shield(id, activity_id).await;
            let broadcast_answer = match &delivery {
                BroadcastDelivery::MaybeDelivered(e) => Some(e.to_string()),
                _ => None,
            };
            let settled = settle_shield(
                &store,
                None,
                wallet_id,
                id,
                &entry,
                &shield_inputs(),
                delivery,
                |e: &dash_sdk::Error| e.to_string(),
                || std::future::ready(Err(wait_error)),
            )
            .await;
            let Err(PlatformWalletError::ShieldedSpendUnconfirmed {
                operation: "shield",
                reason,
            }) = settled
            else {
                panic!("{case}: {settled:?}");
            };
            if let Some(answer) = broadcast_answer {
                assert!(reason.contains(&answer), "{case}: {reason}");
            }
            assert_eq!(
                shield_status(&store, id, &activity_id).await,
                ShieldedActivityStatus::Pending,
                "{case}"
            );
        }
    }

    /// Only a shield whose transition never left the wallet is failed, its
    /// row with it, and nothing waits for its result.
    #[tokio::test]
    async fn should_report_a_shield_failed_only_when_nothing_was_sent() {
        use dash_sdk::dapi_client::DapiClientError;
        use std::cell::Cell;

        let wallet_id = [0x77; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x80; 32];
        let (store, entry) = recorded_shield(id, activity_id).await;
        let waited = Cell::new(false);
        let settled = settle_shield(
            &store,
            None,
            wallet_id,
            id,
            &entry,
            &shield_inputs(),
            BroadcastDelivery::NeverDelivered(dash_sdk::Error::DapiClientError(
                DapiClientError::NoAvailableAddresses,
            )),
            |e: &dash_sdk::Error| e.to_string(),
            || {
                waited.set(true);
                std::future::ready(Err(wait_timeout()))
            },
        )
        .await;
        assert!(
            matches!(
                settled,
                Err(PlatformWalletError::ShieldedBroadcastFailed(_))
            ),
            "{settled:?}"
        );
        assert!(!waited.get(), "nothing was sent, so nothing is waited for");
        assert_eq!(
            shield_status(&store, id, &activity_id).await,
            ShieldedActivityStatus::Failed
        );
    }

    /// A shield succeeds once its verified result reads every input address
    /// at or past the nonce it signed, whatever its broadcast was answered
    /// with. Its row stays pending: the result proves the input addresses,
    /// not this shield or its note, which the scan confirms.
    #[tokio::test]
    async fn should_accept_a_shield_whose_inputs_read_spent_and_leave_its_row_to_the_scan() {
        let wallet_id = [0x78; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x81; 32];
        for (delivery, proved_nonce) in [
            (BroadcastDelivery::Admitted, SHIELD_NONCE),
            (BroadcastDelivery::Admitted, SHIELD_NONCE + 1),
            (
                BroadcastDelivery::MaybeDelivered(unavailable()),
                SHIELD_NONCE,
            ),
            (
                BroadcastDelivery::MaybeDelivered(nonce_verdict(false)),
                SHIELD_NONCE,
            ),
        ] {
            let (store, entry) = recorded_shield(id, activity_id).await;
            settle_shield(
                &store,
                None,
                wallet_id,
                id,
                &entry,
                &shield_inputs(),
                delivery,
                |e: &dash_sdk::Error| e.to_string(),
                || std::future::ready(Ok(input_snapshot(Some(proved_nonce)))),
            )
            .await
            .expect("its inputs read spent");
            assert_eq!(
                shield_status(&store, id, &activity_id).await,
                ShieldedActivityStatus::Pending,
                "nonce {proved_nonce}"
            );
        }
    }

    /// A verified result that does not read the shield's inputs spent shows
    /// the shield had not executed at its block, whatever the endpoint that
    /// returned it claims: a result of another kind, one that leaves an input
    /// out or proves no state for it, and an input still below the nonce the
    /// shield signed leave it unconfirmed, its row pending.
    #[tokio::test]
    async fn should_report_a_shield_unconfirmed_when_its_result_does_not_read_its_inputs_spent() {
        let wallet_id = [0x7A; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x83; 32];
        let cases = [
            (
                "a result of another kind",
                StateTransitionProofResult::VerifiedTokenBalanceAbsence(Identifier::from([1; 32])),
            ),
            (
                "an input left out",
                StateTransitionProofResult::VerifiedAddressInfos(BTreeMap::from([(
                    PlatformAddress::P2pkh([8; 20]),
                    Some((SHIELD_NONCE, 1_000_000)),
                )])),
            ),
            ("an input with no state", input_snapshot(None)),
            (
                "an input below its signed nonce",
                input_snapshot(Some(SHIELD_NONCE - 1)),
            ),
        ];
        for (case, snapshot) in cases {
            let (store, entry) = recorded_shield(id, activity_id).await;
            let settled = settle_shield(
                &store,
                None,
                wallet_id,
                id,
                &entry,
                &shield_inputs(),
                BroadcastDelivery::Admitted,
                |e: &dash_sdk::Error| e.to_string(),
                || std::future::ready(Ok(snapshot)),
            )
            .await;
            assert!(
                matches!(
                    settled,
                    Err(PlatformWalletError::ShieldedSpendUnconfirmed {
                        operation: "shield",
                        ..
                    })
                ),
                "{case}: {settled:?}"
            );
            assert_eq!(
                shield_status(&store, id, &activity_id).await,
                ShieldedActivityStatus::Pending,
                "{case}"
            );
        }

        // A transition that names no input shows nothing spent.
        let (store, entry) = recorded_shield(id, activity_id).await;
        let settled = settle_shield(
            &store,
            None,
            wallet_id,
            id,
            &entry,
            &BTreeMap::new(),
            BroadcastDelivery::Admitted,
            |e: &dash_sdk::Error| e.to_string(),
            || std::future::ready(Ok(input_snapshot(Some(SHIELD_NONCE)))),
        )
        .await;
        assert!(
            matches!(
                settled,
                Err(PlatformWalletError::ShieldedSpendUnconfirmed { .. })
            ),
            "no input: {settled:?}"
        );
    }

    /// The broadcast of a signed shield through a network SDK: a node that
    /// refuses the connection leaves it unconfirmed and its row pending, as a
    /// transport failure cannot show that nothing was sent, and an SDK with
    /// no address to send to leaves it failed.
    #[tokio::test]
    async fn should_report_a_shield_unconfirmed_when_its_node_refuses_the_connection() {
        let wallet_id = [0x79; 32];
        let id = SubwalletId::new(wallet_id, 0);
        let activity_id = [0x82; 32];
        let transition = shield_transition();

        let (store, entry) = recorded_shield(id, activity_id).await;
        let (refusing, _port) = sdk_refusing_connections();
        let refused = broadcast_shield(
            &Arc::new(refusing),
            &store,
            None,
            wallet_id,
            id,
            &entry,
            &transition,
            |e: &dash_sdk::Error| e.to_string(),
        )
        .await;
        assert!(
            matches!(
                refused,
                Err(PlatformWalletError::ShieldedSpendUnconfirmed {
                    operation: "shield",
                    ..
                })
            ),
            "{refused:?}"
        );
        assert_eq!(
            shield_status(&store, id, &activity_id).await,
            ShieldedActivityStatus::Pending
        );

        let (store, entry) = recorded_shield(id, activity_id).await;
        let never_sent = broadcast_shield(
            &Arc::new(sdk_without_addresses()),
            &store,
            None,
            wallet_id,
            id,
            &entry,
            &transition,
            |e: &dash_sdk::Error| e.to_string(),
        )
        .await;
        assert!(
            matches!(
                never_sent,
                Err(PlatformWalletError::ShieldedBroadcastFailed(_))
            ),
            "{never_sent:?}"
        );
        assert_eq!(
            shield_status(&store, id, &activity_id).await,
            ShieldedActivityStatus::Failed
        );
    }
}

#[cfg(test)]
mod nullifier_status_and_claim_record_tests {
    use super::*;
    use crate::wallet::shielded::file_store::FileBackedShieldedStore;
    use crate::wallet::shielded::store::InMemoryShieldedStore;
    use dash_sdk::query_types::ShieldedNullifierStatus;

    fn status(nullifier: [u8; 32], is_spent: bool) -> ShieldedNullifierStatus {
        ShieldedNullifierStatus {
            nullifier,
            is_spent,
        }
    }

    /// Any spent entry wins regardless of coverage: `Spent` is positive proof.
    #[test]
    fn classify_any_spent_is_spent() {
        let queried = [[1u8; 32], [2u8; 32]];
        let statuses = vec![status([1u8; 32], false), status([2u8; 32], true)];
        assert_eq!(
            classify_nullifier_statuses(&statuses, &queried),
            NullifierSpentStatus::Spent
        );
    }

    /// All queried nullifiers covered and none spent — proven unspent.
    #[test]
    fn classify_full_coverage_unspent_is_unspent() {
        let queried = [[1u8; 32], [2u8; 32]];
        let statuses = vec![status([1u8; 32], false), status([2u8; 32], false)];
        assert_eq!(
            classify_nullifier_statuses(&statuses, &queried),
            NullifierSpentStatus::Unspent
        );
    }

    /// A response that omits a queried nullifier proves nothing about it:
    /// partial coverage must NOT read as `Unspent` — that is the path that
    /// would misreport an applied chargeable fallback as a retryable
    /// non-execution (#4204 review finding 8d020115b274).
    #[test]
    fn classify_partial_coverage_is_unknown() {
        let queried = [[1u8; 32], [2u8; 32]];
        let statuses = vec![status([1u8; 32], false)];
        assert_eq!(
            classify_nullifier_statuses(&statuses, &queried),
            NullifierSpentStatus::Unknown
        );
        assert_eq!(
            classify_nullifier_statuses(&[], &queried),
            NullifierSpentStatus::Unknown
        );
    }

    /// The record key is deterministic per one-time key (a retry must find the
    /// record a crashed attempt armed) and distinct across keys.
    #[test]
    fn claim_record_key_is_deterministic_and_distinct() {
        use grovedb_commitment_tree::{FullViewingKey, SpendingKey};

        let fvk = |b: u8| {
            let sk = Option::<SpendingKey>::from(SpendingKey::from_bytes([b; 32]))
                .expect("test byte pattern must be a valid spending key");
            FullViewingKey::from(&sk)
        };
        let a = fvk(1);
        let b = fvk(2);
        assert_eq!(one_time_claim_record_key(&a), one_time_claim_record_key(&a));
        assert_ne!(one_time_claim_record_key(&a), one_time_claim_record_key(&b));
    }

    /// Arm → find → clear round-trip through the reserved claim-records
    /// subwallet, and `finalize_one_time_claim_record`'s settlement rule:
    /// terminal `ShieldedInviteAlreadyClaimed` clears the record, while
    /// `ShieldedBroadcastUnconfirmed` — the outcome whose retry NEEDS the
    /// record — keeps it (#4204 review finding c0781f9d387f).
    #[tokio::test]
    async fn claim_record_round_trip_and_finalize_rules() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id = [7u8; 32];
        let id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        let key = [0xA5u8; 32];

        assert!(find_one_time_claim_record(&store, id, key)
            .await
            .expect("lookup must succeed")
            .is_none());

        {
            let mut guard = store.write().await;
            guard
                .arm_redrive(
                    id,
                    PendingRedrive {
                        activity_id: key,
                        anchor: [9u8; 32],
                        nullifiers: vec![[3u8; 32]],
                        st_bytes: vec![1, 2, 3],
                        attempts: 0,
                        identity_index: None,
                        identity_nonce_finalized: false,
                        identity_user_abandoned: false,
                        anchor_height: None,
                    },
                )
                .expect("arm must succeed");
        }
        let found = find_one_time_claim_record(&store, id, key)
            .await
            .expect("lookup must succeed")
            .expect("armed record must be found");
        assert_eq!(found.nullifiers, vec![[3u8; 32]]);

        // Unconfirmed keeps the record — its retry needs the declared id.
        let unconfirmed: Result<(Identifier, Identity), PlatformWalletError> =
            Err(PlatformWalletError::ShieldedBroadcastUnconfirmed {
                identity_id: Identifier::new([1u8; 32]),
                reason: "test".to_string(),
            });
        finalize_one_time_claim_record(&store, id, key, &unconfirmed).await;
        assert!(find_one_time_claim_record(&store, id, key)
            .await
            .expect("lookup must succeed")
            .is_some());

        // Terminal AlreadyClaimed settles it.
        let terminal: Result<(Identifier, Identity), PlatformWalletError> =
            Err(PlatformWalletError::ShieldedInviteAlreadyClaimed {
                reason: "test".to_string(),
            });
        finalize_one_time_claim_record(&store, id, key, &terminal).await;
        assert!(find_one_time_claim_record(&store, id, key)
            .await
            .expect("lookup must succeed")
            .is_none());
    }

    /// A SUCCESSFUL claim must RETAIN its recovery record until the caller
    /// acknowledges durable local registration (#4313 review finding
    /// 325ce9fa8f84).
    ///
    /// It used to clear on success, which dropped the row before control
    /// returned through `PlatformWallet::identity_create_from_one_time_key` —
    /// where the identity is added to the manager and handed to the persister.
    /// A process death in that gap, a missing wallet-manager entry, or a
    /// `persister.store` failure therefore lost the exact padded identity ID
    /// before the JNI caller ever received it, and the retry could only return
    /// the terminal `ShieldedInviteAlreadyClaimed`.
    #[tokio::test]
    async fn a_successful_claim_retains_its_record_until_registration_is_acknowledged() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0x7Bu8; 32], ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        let key = [0xB6u8; 32];

        store
            .write()
            .await
            .arm_redrive(
                id,
                PendingRedrive {
                    activity_id: key,
                    anchor: [9u8; 32],
                    nullifiers: vec![[3u8; 32]],
                    st_bytes: vec![1, 2, 3],
                    attempts: 0,
                    identity_index: Some(4),
                    identity_nonce_finalized: false,
                    identity_user_abandoned: false,
                    anchor_height: None,
                },
            )
            .expect("arm must succeed");

        let success: Result<(Identifier, Identity), PlatformWalletError> = Ok((
            Identifier::new([0xEE; 32]),
            Identity::default_versioned(dpp::version::PlatformVersion::latest())
                .expect("default identity"),
        ));

        // The claim resolves. This is the instant the native call returns to
        // the host — and the row must still be there.
        let receipt = finalize_one_time_claim_record(&store, id, key, &success).await;
        assert!(
            receipt.is_some(),
            "a successful claim must hand back a record for the caller to acknowledge"
        );
        assert!(
            find_one_time_claim_record(&store, id, key)
                .await
                .expect("lookup must succeed")
                .is_some(),
            "the recovery record must survive the native return — it is the only durable copy of \
             a padded single-note claim's identity id"
        );

        // Registration succeeded and is durable: only NOW is it released.
        acknowledge_one_time_claim_registration(&store, receipt.expect("receipt")).await;
        assert!(
            find_one_time_claim_record(&store, id, key)
                .await
                .expect("lookup must succeed")
                .is_none(),
            "the acknowledgement is what finally drops the record"
        );
    }

    /// The half that makes the retention worth having: a claim killed between
    /// its transition succeeding and the host durably registering the identity
    /// must be RECOVERABLE, not terminal (#4313 review finding 325ce9fa8f84).
    ///
    /// The claim resolves, the record is retained, and then the process dies
    /// before acknowledging — modelled by dropping the receipt and reopening
    /// the store from disk, which is exactly what a relaunch does. The retry
    /// then finds the record, and with it the DECLARED identity id that a
    /// padded single-note bundle cannot re-derive. That id is what
    /// `recover_executed_one_time_claim` needs; without it, the retry's
    /// `expected_identity_id` is `None` and the spent-note path returns the
    /// terminal `ShieldedInviteAlreadyClaimed` before the master-key lookup is
    /// ever attempted.
    #[tokio::test]
    async fn a_claim_killed_before_acknowledgement_is_recoverable_after_a_restart() {
        use dpp::serialization::PlatformSerializable;

        let path = std::env::temp_dir().join(format!(
            "shielded_claim_ack_{}_{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let wallet_id = [0x9Cu8; 32];
        let id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        let key = [0xC7u8; 32];

        // A claim whose transition carries a padded, non-re-derivable id: the
        // single-spend case, where the record is the only handle to it.
        let declared_id = Identifier::new([0x2B; 32]);
        let st_bytes = {
            use dpp::state_transition::identity_create_from_shielded_pool_transition::v0::IdentityCreateFromShieldedPoolTransitionV0;
            use dpp::state_transition::identity_create_from_shielded_pool_transition::IdentityCreateFromShieldedPoolTransition;

            let transition: IdentityCreateFromShieldedPoolTransition =
                IdentityCreateFromShieldedPoolTransitionV0 {
                    public_keys: Vec::new(),
                    denomination: 10_000_000_000,
                    actions: Vec::new(),
                    anchor: [0x07; 32],
                    proof: vec![0x08; 8],
                    binding_signature: [0x09; 64],
                    send_to_address_on_creation_failure: dpp::address_funds::PlatformAddress::P2pkh(
                        [0u8; 20],
                    ),
                    identity_id: declared_id,
                }
                .into();
            StateTransition::IdentityCreateFromShieldedPool(transition)
                .serialize_to_bytes()
                .expect("serialize")
        };

        {
            let store: Arc<RwLock<FileBackedShieldedStore>> = Arc::new(RwLock::new(
                FileBackedShieldedStore::open_path(&path, 100).expect("store opens"),
            ));
            store
                .write()
                .await
                .arm_redrive(
                    id,
                    PendingRedrive {
                        activity_id: key,
                        anchor: [0x0A; 32],
                        nullifiers: vec![[0x0F; 32]],
                        st_bytes,
                        attempts: 0,
                        identity_index: Some(4),
                        identity_nonce_finalized: false,
                        identity_user_abandoned: false,
                        anchor_height: None,
                    },
                )
                .expect("arm must succeed");

            let success: Result<(Identifier, Identity), PlatformWalletError> = Ok((
                declared_id,
                Identity::default_versioned(dpp::version::PlatformVersion::latest())
                    .expect("default identity"),
            ));
            let receipt = finalize_one_time_claim_record(&store, id, key, &success).await;
            assert!(receipt.is_some(), "success yields a receipt");
            // The process dies HERE: the receipt is dropped without ever being
            // acknowledged, so the identity never reached the host's durable
            // state.
            let _dropped_without_acknowledging = receipt;
        }

        // Relaunch: a fresh store over the same file, as `open_path` does on
        // every app start.
        let reopened = FileBackedShieldedStore::open_path(&path, 100).expect("store reopens");
        let rehydrated = reopened.pending_redrives(id).expect("records readable");
        assert_eq!(
            rehydrated.len(),
            1,
            "RED before the fix: the success path cleared the row, so the relaunch found nothing \
             and the retry could only report the invitation as already claimed"
        );

        // And the row still carries the declared id — the value the retry needs
        // and cannot otherwise reconstruct.
        let recovered_id = {
            use dpp::serialization::PlatformDeserializableTrusted;
            use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::accessors::IdentityCreateFromShieldedPoolTransitionAccessorsV0;
            match StateTransition::deserialize_from_bytes_trusted(&rehydrated[0].st_bytes)
                .expect("stored transition deserializes")
            {
                StateTransition::IdentityCreateFromShieldedPool(t) => t.identity_id(),
                other => panic!("unexpected stored transition {}", other.name()),
            }
        };
        assert_eq!(
            recovered_id, declared_id,
            "the retained record is what lets the retry bind a recovered identity to this claim \
             instead of returning terminal ShieldedInviteAlreadyClaimed"
        );

        drop(reopened);
        let _ = std::fs::remove_file(&path);
    }

    /// A corrupt stored transition must not wedge the claim: the resume path
    /// drops the record (so the fresh build proceeds) without touching the
    /// network (the mock SDK has no expectations — any fetch would error).
    #[tokio::test]
    async fn resume_drops_corrupt_record_and_rebuilds() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id = [8u8; 32];
        let id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        let key = [0x5Au8; 32];
        let record = PendingRedrive {
            activity_id: key,
            anchor: [0u8; 32],
            nullifiers: vec![[4u8; 32]],
            st_bytes: vec![0xDE, 0xAD], // never deserializes
            attempts: 0,
            identity_index: None,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
        };
        store
            .write()
            .await
            .arm_redrive(id, record.clone())
            .expect("arm must succeed");

        let outcome = resume_one_time_claim(
            &sdk,
            &store,
            id,
            &record,
            // The corrupt record is rejected long before the
            // pre-broadcast lease gate, so any token reaches the assertion.
            crate::wallet::shielded::store::AdmissionToken::generate().expect("token"),
            None,
            BTreeMap::new(),
            100_000,
            0,
        )
        .await;
        assert!(matches!(outcome, OneTimeClaimResume::RecordUnusable));
        assert!(find_one_time_claim_record(&store, id, key)
            .await
            .expect("lookup must succeed")
            .is_none());
    }
}

#[cfg(test)]
mod classify_spend_wait_failure_tests {
    use super::*;
    use dash_sdk::error::StateTransitionBroadcastError;
    use dpp::consensus::basic::decode::ProtocolVersionParsingError;
    use dpp::consensus::basic::BasicError;
    use dpp::consensus::ConsensusError;

    fn broadcast_err(cause: Option<ConsensusError>) -> dash_sdk::Error {
        dash_sdk::Error::StateTransitionBroadcastError(StateTransitionBroadcastError {
            code: 13,
            message: "context deadline exceeded".to_string(),
            cause,
        })
    }

    /// A broadcast error carrying a consensus cause comes from an
    /// unauthenticated endpoint, with no proof behind it: the spend stays
    /// unconfirmed and its reservation stays.
    #[test]
    fn should_read_a_wait_stage_consensus_verdict_as_unconfirmed() {
        let cause = ConsensusError::BasicError(BasicError::ProtocolVersionParsingError(
            ProtocolVersionParsingError::new("bad version".to_string()),
        ));
        let err = classify_spend_wait_failure("unshield", &broadcast_err(Some(cause)));
        assert!(
            matches!(err, PlatformWalletError::ShieldedSpendUnconfirmed { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn causeless_broadcast_error_is_ambiguous() {
        // DAPI maps its own wait failures (e.g. `DapiError::Timeout`) to a
        // `StateTransitionBroadcastError` with EMPTY consensus data, which the
        // SDK decodes as `cause: None`. The spend may still execute after the
        // wait gave up, so this must NOT release the note reservations.
        let err = classify_spend_wait_failure("unshield", &broadcast_err(None));
        assert!(
            matches!(
                err,
                PlatformWalletError::ShieldedSpendUnconfirmed {
                    operation: "unshield",
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn transport_errors_are_ambiguous() {
        let err = classify_spend_wait_failure(
            "withdraw",
            &dash_sdk::Error::TimeoutReached(
                std::time::Duration::from_secs(80),
                "waiting for response".to_string(),
            ),
        );
        assert!(matches!(
            err,
            PlatformWalletError::ShieldedSpendUnconfirmed {
                operation: "withdraw",
                ..
            }
        ));
    }

    /// The shape a CheckTx rejection takes when it reaches the SDK from
    /// `broadcast()`: DAPI re-attaches the serialized consensus error as
    /// gRPC metadata and the dapi-client decodes it into
    /// `Error::Protocol(ConsensusError)`.
    fn consensus_metadata_rejection() -> dash_sdk::Error {
        let cause = ConsensusError::BasicError(BasicError::ProtocolVersionParsingError(
            ProtocolVersionParsingError::new("bad version".to_string()),
        ));
        dash_sdk::Error::Protocol(dpp::ProtocolError::ConsensusError(Box::new(cause)))
    }

    /// A consensus rejection surfacing from `broadcast()` reads as a refusal
    /// (`broadcast_was_refused`), which proves nothing: the one flow that
    /// reads it, the shield from an identity, reports unconfirmed either
    /// way. On a note spend's wait the same verdict reads as unconfirmed:
    /// no proof backs it.
    #[test]
    fn should_read_a_consensus_rejection_as_a_refusal_and_never_as_a_failed_spend() {
        assert!(broadcast_was_refused(&consensus_metadata_rejection()));
        let err = classify_spend_wait_failure("transfer", &consensus_metadata_rejection());
        assert!(
            matches!(err, PlatformWalletError::ShieldedSpendUnconfirmed { .. }),
            "{err:?}"
        );
    }

    fn grpc_err(code: dash_sdk::dapi_grpc::tonic::Code) -> dash_sdk::Error {
        use dash_sdk::dapi_client::transport::TransportError;
        use dash_sdk::dapi_client::DapiClientError;
        dash_sdk::Error::DapiClientError(DapiClientError::Transport(TransportError::Grpc(
            dash_sdk::dapi_grpc::tonic::Status::new(code, "boom"),
        )))
    }

    /// Connection-establishment failures and non-consensus server
    /// rejections of `broadcast()` read as refusals: the shield from an
    /// identity then skips its result wait for a proven nonce read. None of
    /// them proves the transition was left out of every mempool.
    #[test]
    fn should_read_connection_failures_and_rejections_of_a_broadcast_as_refusals() {
        use dash_sdk::dapi_grpc::tonic::Code;
        assert!(broadcast_was_refused(&grpc_err(Code::Unavailable)));
        // Mempool full / malformed request: server verdicts refusing admission.
        assert!(broadcast_was_refused(&grpc_err(Code::ResourceExhausted)));
        assert!(broadcast_was_refused(&grpc_err(Code::InvalidArgument)));
    }

    /// No-response shapes of the `broadcast()` call itself are not
    /// refusals: the dapi-client retries across addresses, so the request
    /// may have been delivered to a node whose ACK was lost. The caller
    /// falls through to the result wait.
    #[test]
    fn broadcast_no_response_shapes_are_inconclusive() {
        use dash_sdk::dapi_grpc::tonic::Code;
        assert!(!broadcast_was_refused(&grpc_err(Code::DeadlineExceeded)));
        // DAPI maps its own tenderdash-side failures (which can postdate
        // delivery) to Internal.
        assert!(!broadcast_was_refused(&grpc_err(Code::Internal)));
        assert!(!broadcast_was_refused(&dash_sdk::Error::TimeoutReached(
            std::time::Duration::from_secs(30),
            "broadcast".to_string(),
        )));
        assert!(!broadcast_was_refused(&dash_sdk::Error::Generic(
            "transport error: connection reset".to_string()
        )));
        // …including as the terminal error of an exhausted retry loop.
        assert!(!broadcast_was_refused(
            &dash_sdk::Error::NoAvailableAddressesToRetry(Box::new(grpc_err(
                Code::DeadlineExceeded
            )))
        ));
    }

    /// `AlreadyExists` from `broadcast()` proves the transition IS already
    /// in the mempool or on chain (e.g. an internal dapi-client retry after
    /// an ambiguous first delivery), so it must NOT be treated as a failure
    /// — the spend is in flight and the result wait will confirm it.
    #[test]
    fn broadcast_already_exists_is_in_flight() {
        assert!(!broadcast_was_refused(&dash_sdk::Error::AlreadyExists(
            "state transition already in mempool".to_string()
        )));
    }

    /// A consensus verdict wrapped in the dapi-client's `NoAvailableAddressesToRetry`
    /// retry envelope must still count as a rejection — `carries_consensus_rejection`
    /// recurses through the envelope, in lockstep with `as_address_invalid_nonce`.
    #[test]
    fn wrapped_consensus_rejection_is_a_rejection() {
        let wrapped =
            dash_sdk::Error::NoAvailableAddressesToRetry(Box::new(consensus_metadata_rejection()));
        assert!(carries_consensus_rejection(&wrapped));
    }

    /// A transport error wrapped in the retry envelope carries no consensus
    /// verdict, so it remains ambiguous — the recursion must not misread it.
    #[test]
    fn wrapped_transport_error_is_not_a_rejection() {
        use dash_sdk::dapi_grpc::tonic::Code;
        let wrapped = dash_sdk::Error::NoAvailableAddressesToRetry(Box::new(grpc_err(
            Code::DeadlineExceeded,
        )));
        assert!(!carries_consensus_rejection(&wrapped));
    }

    /// A nonce rejection on a note spend's wait, wrapped in the retry
    /// envelope, is an unauthenticated verdict too: unconfirmed, the notes
    /// kept, never a typed refusal a host could retry on.
    #[test]
    fn should_read_a_wrapped_nonce_rejection_on_the_wait_as_unconfirmed() {
        use dpp::address_funds::PlatformAddress;
        use dpp::consensus::state::address_funds::AddressInvalidNonceError;
        use dpp::consensus::state::state_error::StateError;

        let address = PlatformAddress::P2pkh([9u8; 20]);
        let cause = ConsensusError::StateError(StateError::AddressInvalidNonceError(
            AddressInvalidNonceError::new(address, 7, 8),
        ));
        let inner = dash_sdk::Error::Protocol(dpp::ProtocolError::ConsensusError(Box::new(cause)));
        let wrapped = dash_sdk::Error::NoAvailableAddressesToRetry(Box::new(inner));

        let err = classify_spend_wait_failure("withdraw", &wrapped);
        assert!(
            matches!(err, PlatformWalletError::ShieldedSpendUnconfirmed { .. }),
            "{err:?}"
        );
    }
}

#[cfg(test)]
mod shield_input_fetch_error_tests {
    use super::*;
    use dpp::consensus::state::address_funds::AddressNotEnoughFundsError;

    #[test]
    fn live_address_shortfall_maps_to_typed_shield_capacity_error() {
        let sdk_error = dash_sdk::Error::from(AddressNotEnoughFundsError::new(
            PlatformAddress::P2pkh([7; 20]),
            3_623_849_220,
            3_623_849_221,
        ));

        let mapped = map_shield_input_fetch_error(&sdk_error);
        assert!(matches!(
            &mapped,
            PlatformWalletError::PlatformShieldCapacityExceeded { available, required }
                if *available == 3_623_849_220 && *required == 3_623_849_221
        ));
        assert_eq!(
            mapped.to_string(),
            "Platform shield capacity exceeded: available 3623849220, required 3623849221"
        );
    }
}

#[cfg(test)]
mod reserve_shield_fee_tests {
    use super::*;
    use dpp::version::LATEST_PLATFORM_VERSION;

    fn addr(b: u8) -> PlatformAddress {
        PlatformAddress::P2pkh([b; 20])
    }

    #[test]
    fn loads_fee_onto_smallest_key_input() {
        // Input 0 is the BTreeMap-smallest address (addr(1)); the fee must
        // land there, matching the `DeductFromInput(0)` fee strategy.
        let mut inputs = BTreeMap::new();
        inputs.insert(addr(2), 5_000_000u64);
        inputs.insert(addr(1), 1_000_000u64);

        let fee = 123_097_600u64;
        let out = reserve_shield_fee_on_input_0(inputs, fee).expect("non-empty inputs");

        assert_eq!(out.get(&addr(1)), Some(&(1_000_000 + fee)));
        assert_eq!(
            out.get(&addr(2)),
            Some(&5_000_000),
            "other inputs untouched"
        );
        // Σ claims grew by exactly `fee`, satisfying `Σ inputs >= amount + F`.
        assert_eq!(out.values().sum::<u64>(), 6_000_000 + fee);
    }

    #[test]
    fn versioned_fee_keeps_input_zero_valid_and_reserve_tracks_the_fee() {
        let min_input_amount = LATEST_PLATFORM_VERSION
            .dpp
            .state_transitions
            .address_funds
            .min_input_amount;
        let shield_fee = compute_minimum_shielded_fee(SHIELD_NUM_ACTIONS, LATEST_PLATFORM_VERSION)
            .expect("latest shield fee must be computable");
        let reserve = shield_fee_reserve_credits(LATEST_PLATFORM_VERSION)
            .expect("latest shield fee reserve must be computable");
        let smallest_fee_inclusive_claim = shield_fee
            .checked_add(1)
            .expect("latest shield fee plus one credit must fit");

        assert!(
            smallest_fee_inclusive_claim >= min_input_amount,
            "adding the fee must lift even input 0's smallest positive base claim above the protocol minimum"
        );
        assert!(
            reserve >= shield_fee,
            "the retained input-0 headroom must cover the versioned shield fee"
        );
        assert!(
            reserve <= shield_fee.saturating_mul(4),
            "the reserve must stay a small multiple of the versioned fee — an oversized \
             reserve silently understates preflight capacity and strands the excess \
             below the input-0 viability threshold after a Max shield"
        );
    }

    #[test]
    fn errors_on_empty_inputs() {
        let inputs: BTreeMap<PlatformAddress, Credits> = BTreeMap::new();
        let err = reserve_shield_fee_on_input_0(inputs, 1).expect_err("empty must reject");
        assert!(matches!(err, PlatformWalletError::ShieldedBuildError(_)));
    }

    #[test]
    fn errors_on_claim_plus_fee_overflow() {
        let mut inputs = BTreeMap::new();
        inputs.insert(addr(1), u64::MAX);
        let err = reserve_shield_fee_on_input_0(inputs, 1).expect_err("overflow must reject");
        assert!(matches!(err, PlatformWalletError::ShieldedBuildError(_)));
    }
}

#[cfg(test)]
mod pre_broadcast_panic_guard_tests {
    use super::*;
    use crate::wallet::shielded::store::InMemoryShieldedStore;

    /// A panic inside the guarded build/prove step becomes a DEFINITIVE `ShieldedBuildError`
    /// carrying the operation and the panic message — the class of error whose failure arm
    /// releases the note reservation (see `error_releases_note_reservation`).
    #[test]
    fn prover_panic_maps_to_a_releasing_build_error() {
        let err = catch_pre_broadcast_panic::<()>("transfer_multi", || {
            panic!("halo2 constraint system failure")
        })
        .expect_err("a panicking step must surface as an error");
        match &err {
            PlatformWalletError::ShieldedBuildError(m) => {
                assert!(m.contains("transfer_multi"), "missing operation: {m}");
                assert!(
                    m.contains("halo2 constraint system failure"),
                    "missing panic message: {m}"
                );
                assert!(m.contains("before broadcast"), "missing provenance: {m}");
            }
            other => panic!("expected ShieldedBuildError, got: {other:?}"),
        }
        // int38: v5.1-dev releases through the `NoteReservation` guard, not by error class;
        // `cancel_releases_reserved_notes_for_selection` below pins the release itself.
    }

    /// Owned-String panic payloads (`panic!("{x}")` formatting) are preserved too.
    #[test]
    fn string_panic_payloads_are_preserved() {
        let position = 7u64;
        let err = catch_pre_broadcast_panic::<()>("transfer_multi", || {
            panic!("witness for position {position} out of range")
        })
        .expect_err("a panicking step must surface as an error");
        match err {
            PlatformWalletError::ShieldedBuildError(m) => {
                assert!(m.contains("witness for position 7"), "got: {m}");
            }
            other => panic!("expected ShieldedBuildError, got: {other:?}"),
        }
    }

    /// A non-panicking step passes its value through untouched.
    #[test]
    fn success_passes_through() {
        let out = catch_pre_broadcast_panic("transfer_multi", || 41 + 1)
            .expect("a non-panicking step must succeed");
        assert_eq!(out, 42);
    }

    /// End of the chain the guard exists for: once the definitive error reaches the operation's
    /// failure arm, `cancel_pending` releases the reservation and the notes are SELECTABLE
    /// again — unlike the unwinding path, where the anchor-less reservation was unreleasable
    /// for the process lifetime (`stale_pending_spends` skips unarmed reservations).
    #[tokio::test]
    async fn cancel_releases_reserved_notes_for_selection() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = SubwalletId::new([0xEE; 32], 0);
        let note = ShieldedNote {
            position: 0,
            cmx: [1u8; 32],
            nullifier: [2u8; 32],
            block_height: 10,
            is_spent: false,
            value: 5_000,
            note_data: vec![0u8; 115],
        };
        let reservation = {
            let mut s = store.write().await;
            s.save_note(id, &note).unwrap();
            let reservation =
                mark_notes_pending(&mut *s, &store, id, std::slice::from_ref(&note)).unwrap();
            assert!(
                s.get_unspent_notes(id).unwrap().is_empty(),
                "a reserved note must be excluded from selection"
            );
            reservation
        };

        reservation.cancel().await;

        let unspent = store.read().await.get_unspent_notes(id).unwrap();
        assert_eq!(
            unspent.len(),
            1,
            "a cancelled reservation must make the note selectable again"
        );
    }
}

/// OPERATION-level coverage for the pre-broadcast panic guard: every note-spending operation is
/// driven for real with a prover that panics, and the notes it reserved must be selectable again
/// by the time it returns.
///
/// The helper-level tests above pin the conversion; these pin that each operation actually
/// APPLIES it. They run the real `transfer` / `unshield` / `withdraw` /
/// `identity_create_from_shielded_pool` bodies over a real SQLite commitment tree — so note
/// selection, the anchor probe and the builder all execute for real — with only the recorded-
/// anchor fetch served by the mock SDK. Before the guard was extended to these four paths the
/// prover panic unwound straight out of the operation, skipping the failure arm that calls
/// `cancel_pending`, and each test here failed with that panic instead of an error; the
/// reservation left behind was never armed with an anchor, which `stale_pending_spends` skips,
/// so nothing could release it for the rest of the process lifetime.
#[cfg(test)]
mod operation_pre_broadcast_panic_tests {
    use super::*;
    use crate::wallet::shielded::file_store::FileBackedShieldedStore;
    use dash_sdk::query_types::{NoParamQuery, ShieldedAnchors};
    use dashcore::Network;
    use dpp::identity::identity_public_key::v0::IdentityPublicKeyV0;
    use dpp::identity::{KeyType, Purpose, SecurityLevel};
    use dpp::platform_value::BinaryData;
    use dpp::state_transition::public_key_in_creation::v0::IdentityPublicKeyInCreationV0;
    use grovedb_commitment_tree::{
        ExtractedNoteCommitment, Note, NoteValue, ProvingKey, RandomSeed, Rho,
    };

    const WALLET_ID: WalletId = [0x5A; 32];
    const ACCOUNT: u32 = 0;
    /// 0.6 DASH in credits — comfortably above every amount + fee exercised below, and above
    /// the identity-create denomination, so note selection always succeeds and the operations
    /// reach their build/prove step.
    const NOTE_VALUE: u64 = 60_000_000_000;
    /// 0.1 DASH in credits: a member of the versioned exit-denomination set.
    const DENOMINATION: u64 = 10_000_000_000;

    /// An `OrchardProver` whose proving-key access panics.
    ///
    /// This is the faithful injection point: `prove_and_sign_bundle` evaluates
    /// `prover.proving_key()` after `Builder::build` has fixed the action set and before
    /// `create_proof` — i.e. deep inside the guarded build/prove step and strictly before every
    /// one of these operations broadcasts anything.
    struct PanicProver;

    impl dpp::shielded::builder::OrchardProver for PanicProver {
        fn proving_key(&self) -> &ProvingKey {
            panic!("halo2 proving key unavailable")
        }
    }

    /// Fixed 65-byte proof-of-possession stub for the identity-create key set. The builder
    /// fills (and never verifies) these signatures, and the panic fires before the signing loop
    /// is reached anyway.
    #[derive(Debug)]
    struct DummyIdentitySigner;

    #[async_trait::async_trait]
    impl Signer<IdentityPublicKey> for DummyIdentitySigner {
        async fn sign(
            &self,
            _key: &IdentityPublicKey,
            _data: &[u8],
        ) -> Result<BinaryData, dpp::ProtocolError> {
            Ok(BinaryData::new(vec![0u8; 65]))
        }

        async fn sign_create_witness(
            &self,
            _key: &IdentityPublicKey,
            _data: &[u8],
        ) -> Result<dpp::address_funds::AddressWitness, dpp::ProtocolError> {
            Err(dpp::ProtocolError::ShieldedBuildError(
                "identity PoP signer never creates address witnesses".to_string(),
            ))
        }

        fn can_sign_with(&self, _key: &IdentityPublicKey) -> bool {
            true
        }
    }

    /// One AUTHENTICATION/MASTER ECDSA key in both forms the identity-create builder takes.
    fn key_pair() -> (IdentityPublicKey, IdentityPublicKeyInCreation) {
        let public = IdentityPublicKey::V0(IdentityPublicKeyV0 {
            id: 0,
            purpose: Purpose::AUTHENTICATION,
            security_level: SecurityLevel::MASTER,
            contract_bounds: None,
            key_type: KeyType::ECDSA_SECP256K1,
            read_only: false,
            data: BinaryData::new(vec![0xAB; 33]),
            disabled_at: None,
        });
        let in_creation = IdentityPublicKeyInCreation::V0(IdentityPublicKeyInCreationV0 {
            id: 0,
            key_type: KeyType::ECDSA_SECP256K1,
            purpose: Purpose::AUTHENTICATION,
            security_level: SecurityLevel::MASTER,
            contract_bounds: None,
            read_only: false,
            data: BinaryData::new(vec![0xAB; 33]),
            signature: BinaryData::new(vec![]),
        });
        (public, in_creation)
    }

    /// Serialize an Orchard note into the 115-byte store form.
    ///
    /// Mirrors `sync.rs`'s `serialize_note` (private to that module); [`deserialize_note`] is
    /// the parser the operations run it back through, and `fixture` asserts the round trip so
    /// this copy cannot drift from the canonical layout undetected.
    fn serialize_note(note: &Note) -> Vec<u8> {
        let mut data = Vec::with_capacity(115);
        data.extend_from_slice(&note.recipient().to_raw_address_bytes());
        data.extend_from_slice(&note.value().inner().to_le_bytes());
        data.extend_from_slice(&note.rho().to_bytes());
        data.extend_from_slice(note.rseed().as_bytes());
        data
    }

    /// Unique temp path for a test tree (no `tempfile` dev-dep — the convention `file_store`'s
    /// own tests use).
    fn temp_tree_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after the unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("shielded_panic_guard_{tag}_{nanos}.sqlite"))
    }

    /// A wallet holding exactly one real, witnessable note over a real SQLite commitment tree,
    /// with the mock SDK serving that tree's own root as the single recorded anchor — everything
    /// the operations need to get past note selection and the anchor probe and into build/prove.
    struct Fixture {
        sdk: Arc<dash_sdk::Sdk>,
        store: Arc<RwLock<FileBackedShieldedStore>>,
        keys: OrchardKeySet,
        id: SubwalletId,
        path: std::path::PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    async fn fixture(tag: &str) -> Fixture {
        let keys = OrchardKeySet::from_seed(&[0x42u8; 32], Network::Testnet, ACCOUNT)
            .expect("ZIP-32 derivation from a fixed seed should succeed");
        let id = SubwalletId::new(WALLET_ID, ACCOUNT);

        // A real Orchard note paying this account's own default address, so the builders'
        // `add_spend` accepts it under `keys.full_viewing_key`.
        let rho: Rho =
            Option::from(Rho::from_bytes(&[0u8; 32])).expect("zero is a valid pallas::Base");
        let rseed: RandomSeed =
            Option::from(RandomSeed::from_bytes([1u8; 32], &rho)).expect("valid random seed");
        let note: Note = Option::from(Note::from_parts(
            keys.default_address,
            NoteValue::from_raw(NOTE_VALUE),
            rho,
            rseed,
        ))
        .expect("note commitment should be valid");

        let cmx = ExtractedNoteCommitment::from(note.commitment());
        let note_data = serialize_note(&note);
        assert!(
            deserialize_note(&note_data).is_some(),
            "the test note serialization must round-trip through the parser the operations use"
        );

        let path = temp_tree_path(tag);
        let mut store = FileBackedShieldedStore::open_path(&path, 100).expect("open test tree");
        store
            .append_commitment(&cmx.to_bytes(), true)
            .expect("append the note commitment");
        store.checkpoint_tree(1).expect("checkpoint the tree");
        store
            .save_note(
                id,
                &ShieldedNote {
                    position: 0,
                    cmx: cmx.to_bytes(),
                    nullifier: note.nullifier(&keys.full_viewing_key).to_bytes(),
                    block_height: 1,
                    is_spent: false,
                    value: NOTE_VALUE,
                    note_data,
                },
            )
            .expect("save the note");

        // The anchor each operation derives is the root the note's OWN witness computes
        // (`select_recorded_spends`), so serve exactly that as the recorded set — otherwise the
        // probe fails with `ShieldedNoRecordedAnchor` long before the prover is reached.
        let witness = store
            .witness_at_depth(0, 0)
            .expect("witness query should succeed")
            .expect("a marked, checkpointed note must be witnessable at depth 0");
        let anchor = witness.root(cmx).to_bytes();

        // Pin the newest protocol version: an unpinned mock seeds at its network's protocol
        // FLOOR, whose `shielded_identity_create_denominations` set is still empty, so
        // identity-create would be rejected on the denomination long before the prover ran.
        let mut sdk = dash_sdk::SdkBuilder::new_mock()
            .with_version(PlatformVersion::latest())
            .build()
            .expect("mock sdk should build");
        sdk.mock()
            .expect_fetch::<ShieldedAnchors, NoParamQuery>(
                NoParamQuery {},
                Some(ShieldedAnchors(vec![anchor])),
            )
            .await
            .expect("recorded-anchor expectation should register");

        Fixture {
            sdk: Arc::new(sdk),
            store: Arc::new(RwLock::new(store)),
            keys,
            id,
            path,
        }
    }

    /// The shared assertion: the prover panic surfaced as a DEFINITIVE, reservation-releasing
    /// error naming this operation, and the note it had reserved is selectable again.
    async fn assert_panic_released_the_reservation<T: std::fmt::Debug>(
        fx: &Fixture,
        result: Result<T, PlatformWalletError>,
        operation: &str,
    ) {
        let err = result.expect_err("a panicking prover must surface as an error, not a value");
        match &err {
            PlatformWalletError::ShieldedBuildError(m) => {
                assert!(m.contains(operation), "error must name the operation: {m}");
                assert!(
                    m.contains("before broadcast"),
                    "error must record that the panic was provably pre-broadcast: {m}"
                );
                assert!(
                    m.contains("halo2 proving key unavailable"),
                    "error must carry the prover's panic message — otherwise the build failed \
                     before the prover was ever reached and this test proves nothing: {m}"
                );
            }
            other => panic!("expected a definitive ShieldedBuildError, got {other:?}"),
        }
        // int38: the release is checked directly below (v5.1-dev releases through the
        // `NoteReservation` guard, not by error class).

        let unspent = fx
            .store
            .read()
            .await
            .get_unspent_notes(fx.id)
            .expect("unspent notes");
        assert_eq!(
            unspent.len(),
            1,
            "the reserved note must be SELECTABLE again after a pre-broadcast prover panic: the \
             reservation was never armed with an anchor, and `stale_pending_spends` skips \
             anchor-less entries, so if the operation does not release it here nothing ever can"
        );
    }

    #[tokio::test]
    async fn transfer_prover_panic_releases_the_note_reservation() {
        let fx = fixture("transfer").await;
        let recipient = fx.keys.default_address;
        let result = transfer(
            &fx.sdk,
            &fx.store,
            None,
            WALLET_ID,
            &fx.keys,
            ACCOUNT,
            &recipient,
            1_000_000_000,
            [0u8; 36],
            &PanicProver,
        )
        .await;
        assert_panic_released_the_reservation(&fx, result, "transfer").await;
    }

    #[tokio::test]
    async fn unshield_prover_panic_releases_the_note_reservation() {
        let fx = fixture("unshield").await;
        let result = unshield(
            &fx.sdk,
            &fx.store,
            None,
            WALLET_ID,
            &fx.keys,
            ACCOUNT,
            &PlatformAddress::P2pkh([0xCD; 20]),
            1_000_000_000,
            &PanicProver,
        )
        .await;
        assert_panic_released_the_reservation(&fx, result, "unshield").await;
    }

    #[tokio::test]
    async fn withdraw_prover_panic_releases_the_note_reservation() {
        let fx = fixture("withdraw").await;
        let to = dashcore::Address::dummy(Network::Testnet, 42);
        let result = withdraw(
            &fx.sdk,
            &fx.store,
            None,
            WALLET_ID,
            &fx.keys,
            ACCOUNT,
            &to,
            1_000_000_000,
            1,
            &PanicProver,
        )
        .await;
        assert_panic_released_the_reservation(&fx, result, "withdraw").await;
    }

    /// The async sibling: identity-create's builder is a future, so its panic is caught by
    /// `catch_pre_broadcast_panic_async` rather than the synchronous guard. Same contract.
    #[tokio::test]
    async fn identity_create_prover_panic_releases_the_note_reservation() {
        let fx = fixture("identity_create").await;
        let result = identity_create_from_shielded_pool(
            &fx.sdk,
            &fx.store,
            None,
            WALLET_ID,
            &fx.keys,
            ACCOUNT,
            vec![key_pair()],
            DENOMINATION,
            PlatformAddress::P2pkh([0xCD; 20]),
            &DummyIdentitySigner,
            &PanicProver,
        )
        .await;
        assert_panic_released_the_reservation(&fx, result, "identity_create").await;
    }
}

#[cfg(test)]
mod record_activity_status_tests {
    use super::*;
    use crate::wallet::shielded::activity::{
        ShieldedActivityEntry, ShieldedActivityKind, ShieldedDirection,
    };
    use crate::wallet::shielded::store::InMemoryShieldedStore;

    fn sub() -> SubwalletId {
        SubwalletId::new([0xCC; 32], 0)
    }

    /// The Pending entry a live recorder captures before broadcast.
    fn captured_pending() -> ShieldedActivityEntry {
        ShieldedActivityEntry {
            id: [0xAA; 32],
            kind: ShieldedActivityKind::Shield,
            direction: ShieldedDirection::In,
            amount: 1_000,
            fee: Some(10),
            counterparty: None,
            memo: None,
            block_height: None,
            status: ShieldedActivityStatus::Pending,
            created_at_ms: 1,
            min_note_position: None,
            note_cmxs: vec![[0x01; 32]],
            spent_nullifiers: vec![],
        }
    }

    /// A scan pass that confirmed the row at a real height between the
    /// broadcast and the result-wait must win over the post-wait flip:
    /// the stale captured entry must not overwrite the stored
    /// `Confirmed`-with-height row (neither downgrading it to `Failed`
    /// nor erasing the scan-learned height).
    #[tokio::test]
    async fn flip_does_not_clobber_scan_confirmed_row() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = sub();
        let pending = captured_pending();
        let scan_confirmed = with_status(&pending, ShieldedActivityStatus::Confirmed, Some(777));
        store
            .write()
            .await
            .save_activity(id, &scan_confirmed)
            .unwrap();

        record_activity_status(
            &store,
            None,
            id.wallet_id,
            id,
            &Some(pending),
            ShieldedActivityStatus::Failed,
            None,
        )
        .await;

        let stored = store
            .read()
            .await
            .get_activity_by_entry_id(id, &[0xAA; 32])
            .unwrap()
            .expect("row must still exist");
        assert_eq!(stored.status, ShieldedActivityStatus::Confirmed);
        assert_eq!(stored.block_height, Some(777));
    }

    /// No concurrent scan: the flip applies to the stored Pending row
    /// (and falls back to the captured entry when the store has none),
    /// writing the new status into the in-memory store.
    #[tokio::test]
    async fn flip_applies_when_row_is_still_pending() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = sub();
        let pending = captured_pending();
        store.write().await.save_activity(id, &pending).unwrap();

        record_activity_status(
            &store,
            None,
            id.wallet_id,
            id,
            &Some(pending),
            ShieldedActivityStatus::Confirmed,
            Some(900),
        )
        .await;

        let stored = store
            .read()
            .await
            .get_activity_by_entry_id(id, &[0xAA; 32])
            .unwrap()
            .expect("row must exist");
        assert_eq!(stored.status, ShieldedActivityStatus::Confirmed);
        assert_eq!(stored.block_height, Some(900));
    }

    /// The by-id flip the sync reconcile uses: it knows only the
    /// reservation's stored `activity_id`, so it looks the row up and flips
    /// it — a released stranded spend moves Pending → Failed.
    #[tokio::test]
    async fn status_flip_by_id_flips_pending_to_failed() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = sub();
        let pending = captured_pending();
        store.write().await.save_activity(id, &pending).unwrap();

        record_activity_status_by_id(
            &store,
            None,
            id.wallet_id,
            id,
            &pending.id,
            ShieldedActivityStatus::Failed,
        )
        .await;

        let stored = store
            .read()
            .await
            .get_activity_by_entry_id(id, &pending.id)
            .unwrap()
            .expect("row must exist");
        assert_eq!(stored.status, ShieldedActivityStatus::Failed);
    }

    /// A by-id flip for an entry that doesn't exist is a silent no-op
    /// (nothing to flip), never a panic.
    #[tokio::test]
    async fn status_flip_by_id_missing_entry_is_noop() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let id = sub();
        record_activity_status_by_id(
            &store,
            None,
            id.wallet_id,
            id,
            &[0xDE; 32],
            ShieldedActivityStatus::Failed,
        )
        .await;
        assert!(store
            .read()
            .await
            .get_activity_by_entry_id(id, &[0xDE; 32])
            .unwrap()
            .is_none());
    }
}

/// Unit tests for the pure anchor-selection probe ([`select_recorded_spends`])
/// against a real SQLite-backed commitment tree — no SDK, no network.
///
/// These pin the fix for the shielded-withdrawal "never lands" root cause:
/// the wallet must build a spend against a Platform-recorded anchor, not the
/// bleeding-edge depth-0 root a mid-block index-chunk sync leaves behind. They
/// reuse the block-boundary tree shape from the `file_store` reproduction test.
#[cfg(test)]
mod select_recorded_spends_tests {
    use super::*;
    use crate::wallet::shielded::file_store::FileBackedShieldedStore;
    use dashcore::Network;
    use grovedb_commitment_tree::{ExtractedNoteCommitment, Note, NoteValue, RandomSeed, Rho};

    /// Unique temp path for a test tree (no `tempfile` dev-dep).
    fn temp_tree_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("select_recorded_spends_{tag}_{nanos}.sqlite"))
    }

    /// A filler leaf commitment for non-owned positions. Any canonical 32-byte
    /// field element works — the probe only needs the tree to grow between
    /// blocks so successive checkpoint roots differ.
    fn filler_cmx(b: u8) -> [u8; 32] {
        let mut c = [0u8; 32];
        c[0] = b;
        c
    }

    /// Build one real, spendable Orchard note owned by a fixed test seed and
    /// return the wallet's `ShieldedNote` view of it.
    ///
    /// `note_data` is the real serialized note so `deserialize_note` accepts
    /// it, and `cmx` is the note's real extracted commitment so that appending
    /// `cmx` as the leaf at `position` makes `witness(position, d).root(cmx)`
    /// reproduce the tree's anchor at depth `d`.
    fn real_note(position: u64) -> ShieldedNote {
        let keys = OrchardKeySet::from_seed(&[0x42; 32], Network::Testnet, 0)
            .expect("ZIP-32 derivation from a fixed seed");
        let recipient = keys.default_address;

        // rho and rseed must be canonical Pallas base-field elements; not every
        // 32-byte pattern is, so scan deterministically for a valid pair drawn
        // from disjoint byte regions (mirroring the sync tests' note builders).
        let rho = (1u16..=u16::MAX)
            .find_map(|n| {
                let mut b = [0u8; 32];
                b[0..2].copy_from_slice(&n.to_le_bytes());
                Rho::from_bytes(&b).into_option()
            })
            .expect("a canonical rho exists");
        let rseed = (1u16..=u16::MAX)
            .find_map(|m| {
                let mut b = [0u8; 32];
                b[2..4].copy_from_slice(&m.to_le_bytes());
                RandomSeed::from_bytes(b, &rho).into_option()
            })
            .expect("a canonical rseed exists");

        let value = NoteValue::from_raw(100_000);
        let note = Note::from_parts(recipient, value, rho, rseed)
            .into_option()
            .expect("valid note parts");
        let cmx = ExtractedNoteCommitment::from(note.commitment()).to_bytes();

        // `recipient(43) || value(8 LE) || rho(32) || rseed(32)` — the exact
        // format `deserialize_note` expects.
        let mut note_data = Vec::with_capacity(115);
        note_data.extend_from_slice(&note.recipient().to_raw_address_bytes());
        note_data.extend_from_slice(&note.value().inner().to_le_bytes());
        note_data.extend_from_slice(&note.rho().to_bytes());
        note_data.extend_from_slice(note.rseed().as_bytes());

        ShieldedNote {
            position,
            cmx,
            nullifier: [0x07; 32],
            block_height: 1,
            is_spent: false,
            value: 100_000,
            note_data,
        }
    }

    /// Mid-block: the wallet's depth-0 root is not recorded, but a prior
    /// block-boundary checkpoint is — the probe must select that older recorded
    /// anchor (the shallowest one), never the mid-block depth-0 root Platform
    /// never recorded.
    #[test]
    fn mid_block_selects_prior_recorded_checkpoint_not_depth0() {
        let path = temp_tree_path("midblock");
        let mut store = FileBackedShieldedStore::open_path(&path, 100).unwrap();

        // The owned note lives at position 0, present since block 1.
        let note = real_note(0);

        // Block 1 = positions 0,1,2 (leaf 0 is the owned note's cmx). drive
        // records ONE anchor per block, at block-processing-end.
        store.append_commitment(&note.cmx, true).unwrap();
        store.append_commitment(&filler_cmx(0xA1), true).unwrap();
        store.append_commitment(&filler_cmx(0xA2), true).unwrap();
        store.checkpoint_tree(3).unwrap();
        let root_block1 = store.tree_anchor().unwrap();

        // Block 2 = positions 3,4,5. Its block-end root is the second recorded
        // anchor.
        store.append_commitment(&filler_cmx(0xB1), true).unwrap();
        store.append_commitment(&filler_cmx(0xB2), true).unwrap();
        store.append_commitment(&filler_cmx(0xB3), true).unwrap();
        store.checkpoint_tree(6).unwrap();
        let root_block2 = store.tree_anchor().unwrap();

        // Mid-block: the index-chunk sync appends one more commitment (position
        // 6) and checkpoints there. The depth-0 root is now a state drive never
        // recorded.
        store.append_commitment(&filler_cmx(0xC1), true).unwrap();
        store.checkpoint_tree(7).unwrap();
        let root_depth0 = store.tree_anchor().unwrap();

        // drive's recorded set is exactly the two block-boundary roots.
        let recorded: HashSet<[u8; 32]> = [root_block1, root_block2].into_iter().collect();

        let (spends, anchor) =
            select_recorded_spends(&store, std::slice::from_ref(&note), &recorded)
                .expect("a prior recorded checkpoint covers the owned note");

        let _ = std::fs::remove_file(&path);

        assert_eq!(spends.len(), 1, "the single owned note is spendable");
        assert!(
            recorded.contains(&anchor.to_bytes()),
            "the selected anchor must be a Platform-recorded root"
        );
        assert_eq!(
            anchor.to_bytes(),
            root_block2,
            "must pick the shallowest recorded checkpoint (block 2 / depth 1), not a deeper one"
        );
        assert_ne!(
            anchor.to_bytes(),
            root_depth0,
            "must NOT use the mid-block depth-0 root drive never recorded"
        );
    }

    /// Fully synced: the wallet's depth-0 root IS recorded, so the probe takes
    /// the fast path and returns the depth-0 anchor without walking deeper.
    #[test]
    fn fully_synced_returns_depth0_anchor() {
        let path = temp_tree_path("fastpath");
        let mut store = FileBackedShieldedStore::open_path(&path, 100).unwrap();

        let note = real_note(0);

        // One block, checkpointed exactly on its boundary: depth 0 == a recorded
        // anchor.
        store.append_commitment(&note.cmx, true).unwrap();
        store.append_commitment(&filler_cmx(0xA1), true).unwrap();
        store.append_commitment(&filler_cmx(0xA2), true).unwrap();
        store.checkpoint_tree(3).unwrap();
        let root_depth0 = store.tree_anchor().unwrap();

        let recorded: HashSet<[u8; 32]> = [root_depth0].into_iter().collect();

        let (spends, anchor) =
            select_recorded_spends(&store, std::slice::from_ref(&note), &recorded)
                .expect("depth-0 root is recorded");

        let _ = std::fs::remove_file(&path);

        assert_eq!(spends.len(), 1);
        assert_eq!(
            anchor.to_bytes(),
            root_depth0,
            "the fully-synced fast path returns the depth-0 anchor"
        );
    }

    /// No checkpoint root is recorded: the probe exhausts every depth and
    /// returns the retryable `ShieldedNoRecordedAnchor` — nothing is broadcast.
    #[test]
    fn no_recorded_checkpoint_returns_retryable_error() {
        let path = temp_tree_path("none");
        let mut store = FileBackedShieldedStore::open_path(&path, 100).unwrap();

        let note = real_note(0);

        store.append_commitment(&note.cmx, true).unwrap();
        store.append_commitment(&filler_cmx(0xA1), true).unwrap();
        store.append_commitment(&filler_cmx(0xA2), true).unwrap();
        store.checkpoint_tree(3).unwrap();

        // Platform recorded none of this wallet's checkpoint roots.
        let recorded: HashSet<[u8; 32]> = HashSet::new();

        // `SpendableNote` isn't `Debug`, so match rather than `expect_err`.
        let result = select_recorded_spends(&store, std::slice::from_ref(&note), &recorded);

        let _ = std::fs::remove_file(&path);

        match result {
            Err(PlatformWalletError::ShieldedNoRecordedAnchor(_)) => {}
            Err(other) => {
                panic!("expected ShieldedNoRecordedAnchor, got error: {other:?}")
            }
            Ok(_) => panic!("expected ShieldedNoRecordedAnchor, got Ok"),
        }
    }

    /// A selected note that post-dates the only recorded checkpoint. Depth 0
    /// (which contains the note) isn't recorded; at depth 1 the note is not yet
    /// in the tree, so the probe's early-termination fires (a deeper checkpoint
    /// is older still and can't contain it) and the retryable error is returned
    /// — even though a recorded anchor exists, it doesn't cover this note. This
    /// pins the walk's break arm and the value-selection trade-off (a mid-block
    /// wallet whose selected note is newer than every recorded checkpoint waits
    /// for the next sync rather than spending).
    #[test]
    fn note_newer_than_recorded_checkpoint_breaks_and_returns_retryable_error() {
        let path = temp_tree_path("toonew");
        let mut store = FileBackedShieldedStore::open_path(&path, 100).unwrap();

        // Block 1 = positions 0,1,2 (fillers), checkpointed on its boundary —
        // the only root Platform recorded.
        store.append_commitment(&filler_cmx(0xA0), true).unwrap();
        store.append_commitment(&filler_cmx(0xA1), true).unwrap();
        store.append_commitment(&filler_cmx(0xA2), true).unwrap();
        store.checkpoint_tree(3).unwrap();
        let root_block1 = store.tree_anchor().unwrap();

        // The owned note is appended AFTER block 1 (position 3) and checkpointed:
        // present at depth 0, absent from the block-1 checkpoint (depth 1).
        let note = real_note(3);
        store.append_commitment(&note.cmx, true).unwrap();
        store.checkpoint_tree(4).unwrap();

        let recorded: HashSet<[u8; 32]> = [root_block1].into_iter().collect();

        let result = select_recorded_spends(&store, std::slice::from_ref(&note), &recorded);

        let _ = std::fs::remove_file(&path);

        match result {
            Err(PlatformWalletError::ShieldedNoRecordedAnchor(_)) => {}
            Err(other) => panic!("expected ShieldedNoRecordedAnchor, got error: {other:?}"),
            Ok(_) => panic!("expected ShieldedNoRecordedAnchor, got Ok"),
        }
    }
}

/// Unit tests for the ONE-TIME-key claim path
/// ([`identity_create_from_one_time_key`] / [`super::sync::scan_notes_for_foreign_key`]).
///
/// The full op needs a live SDK note stream, so these cover the network-free
/// pieces the crate ADDS: deriving a note owned by a foreign one-time spending
/// key (the scan's per-note conversion — value / cmx / nullifier / serialization),
/// the exact-equality selection over the transiently-scanned set (exact / over /
/// under / no-note), and witnessing that foreign note against a Platform-recorded
/// anchor in the shared marked tree. The key-agnostic Type-20 BUILD with a
/// foreign key is proven by rs-dpp's own green builder tests
/// (`SpendingKey::from_bytes([..]) → fvk/ask → build … succeeds`).
#[cfg(test)]
mod one_time_key_tests {
    use super::*;
    use crate::wallet::shielded::file_store::FileBackedShieldedStore;
    use dpp::version::PlatformVersion;
    use grovedb_commitment_tree::{
        ExtractedNoteCommitment, FullViewingKey, Note, NoteValue, RandomSeed, Rho, Scope,
        SpendingKey,
    };

    /// Smallest member of the versioned exit-denomination set (0.1 DASH).
    const DENOMINATION: u64 = 10_000_000_000;

    /// A fixed, valid one-time Orchard spending key for the tests.
    const ONE_TIME_SK: [u8; 32] = [0x24; 32];

    fn temp_tree_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("one_time_key_{tag}_{nanos}.sqlite"))
    }

    fn filler_cmx(b: u8) -> [u8; 32] {
        let mut c = [0u8; 32];
        c[0] = b;
        c
    }

    /// The full-viewing key of the one-time spending key.
    fn one_time_fvk() -> FullViewingKey {
        let sk: SpendingKey = Option::from(SpendingKey::from_bytes(ONE_TIME_SK))
            .expect("fixed one-time SK is a valid Orchard SpendingKey");
        FullViewingKey::from(&sk)
    }

    /// Build one real Orchard note OWNED BY the one-time key, shaped exactly as
    /// [`super::sync::scan_notes_for_foreign_key`] would produce it: `cmx` is the
    /// note's real commitment, `nullifier` is derived under the one-time key's
    /// fvk, and `note_data` is the canonical 115-byte serialization.
    fn one_time_note(value: u64, position: u64) -> ShieldedNote {
        let fvk = one_time_fvk();
        let recipient = fvk.address_at(0u32, Scope::External);

        // rho / rseed must be canonical Pallas base-field elements — scan
        // deterministically (mirrors the existing note builders in this file).
        let rho = (1u16..=u16::MAX)
            .find_map(|n| {
                let mut b = [0u8; 32];
                b[0..2].copy_from_slice(&n.to_le_bytes());
                Rho::from_bytes(&b).into_option()
            })
            .expect("a canonical rho exists");
        let rseed = (1u16..=u16::MAX)
            .find_map(|m| {
                let mut b = [0u8; 32];
                b[2..4].copy_from_slice(&m.to_le_bytes());
                RandomSeed::from_bytes(b, &rho).into_option()
            })
            .expect("a canonical rseed exists");

        let note = Note::from_parts(recipient, NoteValue::from_raw(value), rho, rseed)
            .into_option()
            .expect("valid note parts");
        let cmx = ExtractedNoteCommitment::from(note.commitment()).to_bytes();
        let nullifier = note.nullifier(&fvk).to_bytes();

        let mut note_data = Vec::with_capacity(115);
        note_data.extend_from_slice(&note.recipient().to_raw_address_bytes());
        note_data.extend_from_slice(&note.value().inner().to_le_bytes());
        note_data.extend_from_slice(&note.rho().to_bytes());
        note_data.extend_from_slice(note.rseed().as_bytes());

        ShieldedNote {
            position,
            cmx,
            nullifier,
            block_height: 1,
            is_spent: false,
            value,
            note_data,
        }
    }

    /// The scan's per-note conversion is correct: a note owned by the one-time
    /// key round-trips through the wallet's 115-byte serialization, and its
    /// nullifier matches the one derived under that key's fvk (what the scan
    /// stamps). This is the piece [`super::sync::scan_notes_for_foreign_key`]
    /// runs on every discovered note.
    #[test]
    fn foreign_key_note_roundtrips_and_nullifier_matches() {
        let note = one_time_note(DENOMINATION, 0);

        // `note_data` deserializes back to an equal note.
        let decoded = deserialize_note(&note.note_data).expect("serialized note is valid");
        assert_eq!(
            decoded.value().inner(),
            DENOMINATION,
            "value survives round-trip"
        );

        // The stamped nullifier is exactly the one the one-time key's fvk derives.
        let fvk = one_time_fvk();
        assert_eq!(
            note.nullifier,
            decoded.nullifier(&fvk).to_bytes(),
            "stamped nullifier must match the fvk-derived nullifier"
        );

        // The stored cmx is the note's real extracted commitment.
        assert_eq!(
            note.cmx,
            ExtractedNoteCommitment::from(decoded.commitment()).to_bytes(),
            "stored cmx must be the note's real commitment"
        );
    }

    /// Exact-equality selection over the transiently-scanned set: exact funding
    /// (zero change), over-funding (change = excess routed to change_address),
    /// under-funding (typed `ShieldedInsufficientBalance`), and no-note (empty →
    /// `ShieldedNoUnspentNotes`, the op's fail-fast on an unfunded key).
    #[test]
    fn select_for_claim_exact_over_under_and_no_note() {
        let version = PlatformVersion::latest();

        // Exact: one note equal to the denomination → zero change.
        let exact = vec![one_time_note(DENOMINATION, 0)];
        let (sel, total, fee) =
            select_notes_for_denomination(&exact, DENOMINATION, 2, 1, version).expect("exact");
        assert_eq!(sel.len(), 1);
        assert_eq!(total, DENOMINATION);
        assert_eq!(total - DENOMINATION, 0, "exact funding leaves zero change");
        assert!(fee < DENOMINATION, "fee must leave a positive balance");

        // Over-funded: the excess above the denomination becomes the change note.
        let excess = 7_000_000_000u64;
        let over = vec![one_time_note(DENOMINATION + excess, 0)];
        let (sel, total, _) =
            select_notes_for_denomination(&over, DENOMINATION, 2, 1, version).expect("over");
        assert_eq!(sel.len(), 1);
        assert_eq!(
            total - DENOMINATION,
            excess,
            "over-funding routes the excess to change_address"
        );

        // Under-funded: a single note below the denomination.
        let under = vec![one_time_note(DENOMINATION - 1, 0)];
        match select_notes_for_denomination(&under, DENOMINATION, 2, 1, version) {
            Err(PlatformWalletError::ShieldedInsufficientBalance {
                available,
                required,
            }) => {
                assert_eq!(available, DENOMINATION - 1);
                assert_eq!(required, DENOMINATION);
            }
            other => panic!("expected ShieldedInsufficientBalance, got {other:?}"),
        }

        // No note found for the key: empty set → ShieldedNoUnspentNotes (the same
        // error the op raises on `discovered.is_empty()`).
        match select_notes_for_denomination(&[], DENOMINATION, 2, 1, version) {
            Err(PlatformWalletError::ShieldedNoUnspentNotes) => {}
            other => panic!("expected ShieldedNoUnspentNotes, got {other:?}"),
        }
    }

    /// The witness half: a note owned by the one-time key, appended to the shared
    /// fully-marked tree, is witnessable and produces a `SpendableNote` against a
    /// Platform-recorded anchor — the same probe the op runs before the build.
    #[test]
    fn foreign_key_note_witnesses_against_recorded_anchor() {
        let path = temp_tree_path("witness");
        let mut store = FileBackedShieldedStore::open_path(&path, 100).unwrap();

        let note = one_time_note(DENOMINATION, 0);

        // One block, checkpointed on its boundary: depth-0 root is recorded.
        store.append_commitment(&note.cmx, true).unwrap();
        store.append_commitment(&filler_cmx(0xA1), true).unwrap();
        store.append_commitment(&filler_cmx(0xA2), true).unwrap();
        store.checkpoint_tree(3).unwrap();
        let root_depth0 = store.tree_anchor().unwrap();

        let recorded: HashSet<[u8; 32]> = [root_depth0].into_iter().collect();

        let (spends, anchor) =
            select_recorded_spends(&store, std::slice::from_ref(&note), &recorded)
                .expect("the one-time key's note witnesses against the recorded anchor");

        let _ = std::fs::remove_file(&path);

        assert_eq!(
            spends.len(),
            1,
            "the one-time key's single note is spendable"
        );
        assert_eq!(
            spends[0].note.value().inner(),
            DENOMINATION,
            "the witnessed SpendableNote carries the funded value"
        );
        assert_eq!(
            anchor.to_bytes(),
            root_depth0,
            "the spend is built against the Platform-recorded anchor"
        );
    }
}

/// Regression tests for one-time-key (shielded invitation) claim RECOVERY
/// ownership evidence.
///
/// A spent invitation nullifier proves only that *something* consumed the note.
/// It does **not** prove that this claim's Type-20 transition created an
/// identity, and these tests pin the two on-chain outcomes where the pre-fix
/// rule — "the nullifier is spent and an identity is findable under the
/// submitted MASTER auth key hash" — reported a successful claim that never
/// happened.
#[cfg(test)]
mod one_time_claim_evidence_tests {
    use super::*;
    use crate::wallet::shielded::file_store::FileBackedShieldedStore;
    use dpp::identity::identity_public_key::v0::IdentityPublicKeyV0;
    use dpp::identity::{KeyType, Purpose, SecurityLevel};
    use dpp::platform_value::BinaryData;
    use dpp::version::PlatformVersion;

    /// This claim's submitted MASTER auth key hash.
    const OUR_MASTER_HASH: [u8; 20] = [0xA1; 20];
    /// Some other key's hash — used for the competing-claimant identity.
    const OTHER_MASTER_HASH: [u8; 20] = [0xB2; 20];
    /// Smallest member of the versioned exit-denomination set (0.1 DASH).
    const DENOMINATION: u64 = 10_000_000_000;
    /// The local DIP-9 slot the original attempt registered at.
    const IDENTITY_INDEX: u32 = 3;

    fn temp_store_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("one_time_claim_{tag}_{nanos}.sqlite"))
    }

    /// The two real note nullifiers this claim spends.
    fn our_nullifiers() -> Vec<[u8; 32]> {
        vec![[0x11; 32], [0x22; 32]]
    }

    /// An `ECDSA_HASH160` key whose `public_key_hash()` is exactly `hash` —
    /// `KeyType::ECDSA_HASH160` returns its 20-byte `data` verbatim, so the test
    /// controls the hash precisely without generating real key material.
    fn key_with_hash(
        id: u32,
        purpose: Purpose,
        security_level: SecurityLevel,
        hash: [u8; 20],
    ) -> IdentityPublicKey {
        IdentityPublicKey::V0(IdentityPublicKeyV0 {
            id,
            purpose,
            security_level,
            contract_bounds: None,
            key_type: KeyType::ECDSA_HASH160,
            read_only: false,
            data: BinaryData::new(hash.to_vec()),
            disabled_at: None,
        })
    }

    fn identity_with_keys(id: Identifier, keys: Vec<IdentityPublicKey>) -> Identity {
        let map: BTreeMap<u32, IdentityPublicKey> = keys.into_iter().map(|k| (k.id(), k)).collect();
        Identity::new_with_id_and_keys(id, map, PlatformVersion::latest())
            .expect("test identity builds")
    }

    /// The MASTER auth key this claim submits.
    fn our_master_key() -> IdentityPublicKey {
        key_with_hash(
            0,
            Purpose::AUTHENTICATION,
            SecurityLevel::MASTER,
            OUR_MASTER_HASH,
        )
    }

    /// The **pre-fix** acceptance rule, encoded here as the behavior these tests
    /// exist to reject.
    ///
    /// Before the fix, both recovery handles returned `Ok((identity.id(),
    /// identity))` for *whatever* identity the lookup produced — the fetched
    /// identity was never inspected. So the old rule accepted unconditionally
    /// once a lookup succeeded, and every case below that asserts
    /// `recovered_identity_matches_claim(..) == false` is a case the old code
    /// returned as a successful claim.
    fn pre_fix_rule_accepts(_identity: &Identity) -> bool {
        true
    }

    /// BLOCKER 1 — chargeable `UnshieldAction` fallback must not read as success.
    ///
    /// When a submitted unique public-key hash is already registered, Type-20
    /// finalizes the shielded spend as an `UnshieldTransitionAction` with
    /// `chargeable_failure: true`: the nullifier IS consumed, the invitation
    /// value goes to the creation-failure address, and **no identity is
    /// created**. A retry then finds the *pre-existing* identity that owns the
    /// colliding key hash. Its id is not the one this claim's nullifiers derive,
    /// so the id binding must reject it.
    #[test]
    fn chargeable_unshield_fallback_identity_is_rejected() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());

        // The pre-existing identity: it genuinely owns our MASTER key hash (that
        // is exactly why the unique-key-hash collision fired), but it was created
        // by some unrelated earlier transition, so it carries an unrelated id.
        let pre_existing = identity_with_keys(Identifier::from([0xEE; 32]), vec![our_master_key()]);

        assert!(
            pre_existing.id() != expected_id,
            "precondition: the colliding identity is not the one this claim derives"
        );
        assert!(
            pre_fix_rule_accepts(&pre_existing),
            "the pre-fix rule accepted this identity as a successful claim"
        );
        assert!(
            !recovered_identity_matches_claim(
                &pre_existing,
                Some(expected_id),
                Some(OUR_MASTER_HASH)
            ),
            "an identity that merely owns the submitted master auth key hash must NOT be \
             reported as this claim's result: the spend was finalized as a chargeable failure \
             and created no identity"
        );
    }

    /// BLOCKER 2 — a competing holder of the same bearer key must not read as
    /// success.
    ///
    /// The identity id is derived from published nullifiers only, never from
    /// identity keys. With two or more real spends no randomized padding action
    /// is added, so another holder of the same one-time key spending the same
    /// notes derives the SAME id under THEIR keys. The key binding must reject
    /// it — otherwise the foreign identity is registered at this wallet's
    /// caller-supplied identity index.
    #[test]
    fn competing_bearer_key_holder_identity_is_rejected() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());

        // Same notes => same nullifiers => same derived id, but the winner
        // registered their own master key.
        let foreign = identity_with_keys(
            expected_id,
            vec![key_with_hash(
                0,
                Purpose::AUTHENTICATION,
                SecurityLevel::MASTER,
                OTHER_MASTER_HASH,
            )],
        );

        assert_eq!(
            foreign.id(),
            expected_id,
            "precondition: the race winner's identity shares this claim's derived id"
        );
        assert!(
            pre_fix_rule_accepts(&foreign),
            "the pre-fix rule accepted this identity as a successful claim"
        );
        assert!(
            !recovered_identity_matches_claim(&foreign, Some(expected_id), Some(OUR_MASTER_HASH)),
            "an identity at this claim's derived id that does not carry the submitted master \
             auth key belongs to another holder of the one-time key and must NOT be returned"
        );
    }

    /// A keyless fetch must fail closed rather than be topped up with the
    /// locally-submitted keys — those were never proven to exist on chain.
    #[test]
    fn identity_fetched_without_public_keys_is_rejected() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());
        let keyless = identity_with_keys(expected_id, vec![]);

        assert!(
            pre_fix_rule_accepts(&keyless),
            "the pre-fix rule accepted this identity and then inserted the submitted keys locally"
        );
        assert!(
            !recovered_identity_matches_claim(&keyless, Some(expected_id), Some(OUR_MASTER_HASH)),
            "an identity fetched without public keys cannot prove the key binding"
        );
    }

    /// A single-spend claim's id is not re-derivable (the bundle is padded to
    /// Orchard's 2-action minimum with a randomly generated dummy nullifier that
    /// participates in the derivation), so no candidate can ever be bound to it.
    #[test]
    fn unre_derivable_id_is_rejected() {
        let identity = identity_with_keys(Identifier::from([0xEE; 32]), vec![our_master_key()]);

        assert!(
            !recovered_identity_matches_claim(&identity, None, Some(OUR_MASTER_HASH)),
            "without a re-derivable id there is no evidence this claim created the identity"
        );
    }

    /// A missing MASTER auth key hash is not evidence either.
    #[test]
    fn absent_master_key_hash_is_rejected() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());
        let identity = identity_with_keys(expected_id, vec![our_master_key()]);

        assert!(
            !recovered_identity_matches_claim(&identity, Some(expected_id), None),
            "without a submitted master auth key hash the key binding cannot be established"
        );
    }

    /// A key with the right hash but the wrong purpose/security level does not
    /// satisfy the key binding — the binding is specifically on the MASTER
    /// AUTHENTICATION key, which is the uniquely Platform-indexed handle.
    #[test]
    fn non_master_key_with_matching_hash_is_rejected() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());
        let identity = identity_with_keys(
            expected_id,
            vec![
                key_with_hash(
                    0,
                    Purpose::AUTHENTICATION,
                    SecurityLevel::HIGH,
                    OUR_MASTER_HASH,
                ),
                key_with_hash(
                    1,
                    Purpose::TRANSFER,
                    SecurityLevel::CRITICAL,
                    OUR_MASTER_HASH,
                ),
            ],
        );

        assert!(
            !recovered_identity_matches_claim(&identity, Some(expected_id), Some(OUR_MASTER_HASH)),
            "only a MASTER AUTHENTICATION key satisfies the key binding"
        );
    }

    /// The positive case: both bindings hold, so this claim provably created the
    /// identity and recovery returns it.
    #[test]
    fn identity_with_matching_id_and_master_key_is_accepted() {
        let expected_id = identity_id_from_nullifiers(&our_nullifiers());
        let ours = identity_with_keys(
            expected_id,
            vec![
                our_master_key(),
                key_with_hash(1, Purpose::TRANSFER, SecurityLevel::CRITICAL, [0xC3; 20]),
            ],
        );

        assert!(
            recovered_identity_matches_claim(&ours, Some(expected_id), Some(OUR_MASTER_HASH)),
            "an identity carrying this claim's derived id AND its submitted master auth key was \
             created by this claim"
        );
    }

    /// The id binding is only meaningful because the derivation is over the
    /// claim's own nullifier set: a different note selection derives a different
    /// id, so it cannot be passed off as this claim's result.
    #[test]
    fn a_different_nullifier_set_derives_a_different_id() {
        let ours = identity_id_from_nullifiers(&our_nullifiers());
        let theirs = identity_id_from_nullifiers(&[[0x11; 32], [0x33; 32]]);

        assert_ne!(
            ours, theirs,
            "the derived id is a function of the published nullifier set"
        );

        let identity = identity_with_keys(theirs, vec![our_master_key()]);
        assert!(
            !recovered_identity_matches_claim(&identity, Some(ours), Some(OUR_MASTER_HASH)),
            "an identity created from a different nullifier set is not this claim's identity"
        );
    }

    // ── Resumed-claim binding (#4313 review finding 195efdd4ae21) ──────────
    //
    // The pending-claim record is found by wallet id and one-time FVK alone, so
    // the resume must take its binding from the STORED TRANSITION and refuse a
    // retry whose arguments disagree — never act on the caller's values while
    // re-broadcasting someone else's bytes.

    /// Serialize a shielded identity-create transition carrying exactly `keys`
    /// and `denomination`, shaped as `arm_one_time_claim_record` stores it.
    fn stored_claim_transition(
        keys: &[IdentityPublicKey],
        denomination: u64,
    ) -> (StateTransition, Vec<u8>) {
        use dpp::serialization::PlatformSerializable;
        use dpp::state_transition::identity_create_from_shielded_pool_transition::v0::IdentityCreateFromShieldedPoolTransitionV0;
        use dpp::state_transition::identity_create_from_shielded_pool_transition::IdentityCreateFromShieldedPoolTransition;

        let transition: IdentityCreateFromShieldedPoolTransition =
            IdentityCreateFromShieldedPoolTransitionV0 {
                public_keys: keys
                    .iter()
                    .map(|key| IdentityPublicKeyInCreation::from(key.clone()))
                    .collect(),
                denomination,
                actions: Vec::new(),
                anchor: [0x07; 32],
                proof: vec![0x08; 8],
                binding_signature: [0x09; 64],
                send_to_address_on_creation_failure: dpp::address_funds::PlatformAddress::P2pkh(
                    [0u8; 20],
                ),
                identity_id: identity_id_from_nullifiers(&our_nullifiers()),
            }
            .into();
        let st = StateTransition::IdentityCreateFromShieldedPool(transition);
        let bytes = st.serialize_to_bytes().expect("transition serializes");
        (st, bytes)
    }

    /// A pending-claim record over `st_bytes`, keyed like a real one.
    fn stored_claim_record(st_bytes: Vec<u8>) -> PendingRedrive {
        PendingRedrive {
            activity_id: [0x5A; 32],
            anchor: [0x07; 32],
            nullifiers: our_nullifiers(),
            st_bytes,
            attempts: 0,
            identity_index: Some(IDENTITY_INDEX),
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
        }
    }

    fn keys_map(keys: &[IdentityPublicKey]) -> BTreeMap<u32, IdentityPublicKey> {
        keys.iter().map(|key| (key.id(), key.clone())).collect()
    }

    /// A second key set that differs from `our_master_key()` only in the key
    /// MATERIAL — same id, purpose and security level. This is the dangerous
    /// shape: ids alone still line up, so anything comparing only ids would
    /// wave it through and register a foreign identity at this wallet's slot.
    fn other_master_key() -> IdentityPublicKey {
        key_with_hash(
            0,
            Purpose::AUTHENTICATION,
            SecurityLevel::MASTER,
            OTHER_MASTER_HASH,
        )
    }

    /// THE DERIVE PATH: everything the resume needs is recoverable from the
    /// serialized transition, which is why no record-schema change is required.
    /// A round-trip through `StateTransition` must reproduce the exact key set
    /// (by id AND content), the denomination, and the MASTER auth key hash that
    /// idempotent recovery probes Platform with.
    #[test]
    fn claim_binding_is_recoverable_from_the_stored_transition() {
        use dpp::serialization::PlatformDeserializableTrusted;
        use dpp::state_transition::state_transitions::shielded::identity_create_from_shielded_pool_transition::accessors::IdentityCreateFromShieldedPoolTransitionAccessorsV0;

        let submitted = vec![
            our_master_key(),
            key_with_hash(1, Purpose::TRANSFER, SecurityLevel::CRITICAL, [0xC3; 20]),
        ];
        let (_, st_bytes) = stored_claim_transition(&submitted, DENOMINATION);

        let restored = StateTransition::deserialize_from_bytes_trusted(&st_bytes)
            .expect("stored bytes deserialize");
        let StateTransition::IdentityCreateFromShieldedPool(transition) = &restored else {
            panic!("stored record must carry a shielded identity-create transition");
        };

        let derived: BTreeMap<u32, IdentityPublicKey> = transition
            .public_keys()
            .iter()
            .map(|key_in_creation| {
                let key: IdentityPublicKey = key_in_creation.into();
                (key.id(), key)
            })
            .collect();

        assert_eq!(
            derived,
            keys_map(&submitted),
            "the submitted key set must be recoverable from the transition itself"
        );
        assert_eq!(transition.denomination(), DENOMINATION);
        assert_eq!(
            master_auth_public_key_hash_of(derived.values()),
            Some(OUR_MASTER_HASH),
            "the recovery handle must be derivable from the transition, not supplied by the retry"
        );
    }

    /// MATCHING ARGS: a retry presenting exactly what the earlier attempt
    /// submitted is a genuine resume and must pass the binding gate.
    #[test]
    fn matching_retry_arguments_resume() {
        let submitted = vec![our_master_key()];
        let keys = keys_map(&submitted);

        assert_eq!(
            one_time_claim_binding_mismatch(
                &keys,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                Some(IDENTITY_INDEX),
                &keys,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                IDENTITY_INDEX,
            ),
            None,
            "identical arguments must not be treated as a mis-binding"
        );
    }

    /// Key ORDER is not a mismatch: both sides are keyed by key id, so a caller
    /// that assembles the same keys in a different order still resumes.
    #[test]
    fn key_order_is_not_a_binding_mismatch() {
        let forward = keys_map(&[
            our_master_key(),
            key_with_hash(1, Purpose::TRANSFER, SecurityLevel::CRITICAL, [0xC3; 20]),
        ]);
        let reversed = keys_map(&[
            key_with_hash(1, Purpose::TRANSFER, SecurityLevel::CRITICAL, [0xC3; 20]),
            our_master_key(),
        ]);

        assert_eq!(
            one_time_claim_binding_mismatch(
                &forward,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                Some(IDENTITY_INDEX),
                &reversed,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                IDENTITY_INDEX,
            ),
            None
        );
    }

    /// MISMATCHED ARGS, per field. Each of these is a way the pre-fix resume
    /// would have acted on the caller's value while broadcasting the stored
    /// bytes: a swapped key set registers a foreign identity at this wallet's
    /// slot and backfills an empty proof result with keys that were never in the
    /// transition; a swapped master hash makes idempotent recovery probe
    /// Platform for someone else's identity; a swapped denomination misreports
    /// the value that left the pool.
    #[test]
    fn mismatched_retry_arguments_are_refused_per_field() {
        let stored = keys_map(&[our_master_key()]);
        let swapped = keys_map(&[other_master_key()]);

        // Same key ids, different key material — ids alone would not catch it.
        assert_eq!(
            stored.keys().collect::<Vec<_>>(),
            swapped.keys().collect::<Vec<_>>(),
            "precondition: the swap keeps the key ids identical"
        );

        let key_mismatch = one_time_claim_binding_mismatch(
            &stored,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            Some(IDENTITY_INDEX),
            &swapped,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            IDENTITY_INDEX,
        );
        assert!(
            key_mismatch.is_some_and(|m| m.contains("public key set")),
            "a swapped key set must be refused"
        );

        let hash_mismatch = one_time_claim_binding_mismatch(
            &stored,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            Some(IDENTITY_INDEX),
            &stored,
            Some(OTHER_MASTER_HASH),
            DENOMINATION,
            IDENTITY_INDEX,
        );
        assert!(
            hash_mismatch.is_some_and(|m| m.contains("master authentication key hash")),
            "a recovery handle that is not in the stored transition must be refused"
        );

        let denomination_mismatch = one_time_claim_binding_mismatch(
            &stored,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            Some(IDENTITY_INDEX),
            &stored,
            Some(OUR_MASTER_HASH),
            DENOMINATION * 3,
            IDENTITY_INDEX,
        );
        assert!(
            denomination_mismatch.is_some_and(|m| m.contains("denomination")),
            "a different denomination must be refused"
        );
    }

    /// END TO END, and the property that matters most: a mismatched retry must
    /// fail CLOSED — refused with `ShieldedClaimBindingMismatch` **before** any
    /// network work, with the pending record left intact so the correct retry
    /// can still resume. The SDK here is a bare mock with no expectations
    /// registered: reaching the spent-nullifier probe or the re-broadcast would
    /// surface as something other than this error.
    #[tokio::test]
    async fn mismatched_retry_refuses_without_broadcasting_or_clearing_the_record() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let path = temp_store_path("resume_binding");
        let store = Arc::new(RwLock::new(
            FileBackedShieldedStore::open_path(&path, 100).expect("store opens"),
        ));
        let claim_records_id = SubwalletId::new([0x77; 32], ONE_TIME_CLAIM_RECORDS_ACCOUNT);

        let (_, st_bytes) = stored_claim_transition(&[our_master_key()], DENOMINATION);
        let record = stored_claim_record(st_bytes);
        store
            .write()
            .await
            .arm_redrive(claim_records_id, record.clone())
            .expect("record arms");

        // The retry presents a DIFFERENT identity's keys — the mis-slot case.
        let outcome = resume_one_time_claim(
            &sdk,
            &store,
            claim_records_id,
            &record,
            // The binding mismatch is refused before the pre-broadcast
            // lease gate is ever reached.
            crate::wallet::shielded::store::AdmissionToken::generate().expect("token"),
            Some(OTHER_MASTER_HASH),
            keys_map(&[other_master_key()]),
            DENOMINATION,
            IDENTITY_INDEX,
        )
        .await;

        match outcome {
            OneTimeClaimResume::Resolved(Err(
                PlatformWalletError::ShieldedClaimBindingMismatch { mismatch },
            )) => assert!(
                mismatch.contains("master authentication key hash")
                    || mismatch.contains("public key set"),
                "the refusal must name the binding that failed, got: {mismatch}"
            ),
            other => panic!(
                "a retry with a different identity's keys must be refused, got {}",
                match other {
                    OneTimeClaimResume::RecordUnusable => "RecordUnusable".to_string(),
                    OneTimeClaimResume::Resolved(r) => format!("Resolved({r:?})"),
                }
            ),
        }

        // Fail-closed: the record survives, so the ORIGINAL claim is still
        // resumable. Clearing it here would strand a padded single-note claim
        // forever — its declared id exists nowhere else.
        let surviving = store
            .read()
            .await
            .pending_redrives(claim_records_id)
            .expect("records readable");
        assert_eq!(
            surviving.len(),
            1,
            "a refused retry must not clear the pending-claim record"
        );
        assert_eq!(
            surviving[0].st_bytes, record.st_bytes,
            "the stored transition must be untouched"
        );

        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    /// THE BUG (#4313 review finding 5d4d6efa): `identity_index` was bound only
    /// TRANSITIVELY — "a different slot means different keys, so the key check
    /// catches it". A retry that presents the ORIGINAL keys with a different
    /// slot breaks that chain, and its consequence is not symmetric with a
    /// first attempt's: `IdentityManager::add_identity` rejects a duplicate
    /// identity id but inserts into an OCCUPIED slot without complaint, so the
    /// retry silently displaces whatever identity the wallet tracked there.
    ///
    /// The record now carries the slot, so the mismatch is caught with
    /// everything else byte-identical — exactly the case the transitive
    /// argument could not cover.
    #[test]
    fn a_resume_at_a_different_identity_index_is_refused() {
        let keys = keys_map(&[our_master_key()]);

        let mismatch = one_time_claim_binding_mismatch(
            &keys,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            Some(IDENTITY_INDEX),
            // Everything else is identical — only the slot moved.
            &keys,
            Some(OUR_MASTER_HASH),
            DENOMINATION,
            IDENTITY_INDEX + 1,
        );
        let mismatch = mismatch.expect("a slot mismatch must be refused");
        assert!(
            mismatch.contains("identity index"),
            "the refusal must name the slot, got: {mismatch}"
        );
        assert!(
            mismatch.contains(&IDENTITY_INDEX.to_string())
                && mismatch.contains(&(IDENTITY_INDEX + 1).to_string()),
            "the refusal must carry both slots, got: {mismatch}"
        );
    }

    /// A record written before the column existed carries `None`. There is
    /// nothing to compare it against, so it keeps exactly the transitive
    /// binding it was written under rather than being refused outright — an
    /// upgrade must not strand a claim that is mid-flight across it.
    #[test]
    fn a_pre_migration_record_still_resumes_at_any_index() {
        let keys = keys_map(&[our_master_key()]);

        assert_eq!(
            one_time_claim_binding_mismatch(
                &keys,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                None,
                &keys,
                Some(OUR_MASTER_HASH),
                DENOMINATION,
                IDENTITY_INDEX + 7,
            ),
            None,
            "a record with no persisted slot must not be refused on the slot"
        );
    }

    /// END TO END on the real file store: arm a claim at slot N, attempt to
    /// resume it at N+1, and get the typed refusal BEFORE any network work —
    /// with the record left intact for a correct retry. The SDK is a bare mock
    /// with no expectations registered, so reaching the spent-nullifier probe
    /// or the re-broadcast would surface as something other than this error.
    #[tokio::test]
    async fn resuming_at_a_mismatched_identity_index_fails_closed() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let path = temp_store_path("resume_slot_binding");
        let store = Arc::new(RwLock::new(
            FileBackedShieldedStore::open_path(&path, 100).expect("store opens"),
        ));
        let claim_records_id = SubwalletId::new([0x78; 32], ONE_TIME_CLAIM_RECORDS_ACCOUNT);

        let (_, st_bytes) = stored_claim_transition(&[our_master_key()], DENOMINATION);
        let record = stored_claim_record(st_bytes);
        assert_eq!(
            record.identity_index,
            Some(IDENTITY_INDEX),
            "precondition: the armed record carries the slot"
        );
        store
            .write()
            .await
            .arm_redrive(claim_records_id, record.clone())
            .expect("record arms");

        // The retry presents the ORIGINAL keys, hash and denomination — only
        // the slot differs. Nothing but the persisted index can catch this.
        let outcome = resume_one_time_claim(
            &sdk,
            &store,
            claim_records_id,
            &record,
            // As above: refused on the slot mismatch, before the gate.
            crate::wallet::shielded::store::AdmissionToken::generate().expect("token"),
            Some(OUR_MASTER_HASH),
            keys_map(&[our_master_key()]),
            DENOMINATION,
            IDENTITY_INDEX + 1,
        )
        .await;

        match outcome {
            OneTimeClaimResume::Resolved(Err(
                PlatformWalletError::ShieldedClaimBindingMismatch { mismatch },
            )) => assert!(
                mismatch.contains("identity index"),
                "the refusal must name the slot, got: {mismatch}"
            ),
            other => panic!(
                "a retry at a different slot must be refused, got {}",
                match other {
                    OneTimeClaimResume::RecordUnusable => "RecordUnusable".to_string(),
                    OneTimeClaimResume::Resolved(r) => format!("Resolved({r:?})"),
                }
            ),
        }

        // Fail-closed: the record survives for a retry that names slot N.
        let surviving = store
            .read()
            .await
            .pending_redrives(claim_records_id)
            .expect("records readable");
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].identity_index, Some(IDENTITY_INDEX));

        // …and the slot is DURABLE, not just in-memory: a cold reopen still
        // carries it, which is the whole point of the schema change.
        drop(store);
        let reopened = FileBackedShieldedStore::open_path(&path, 100).expect("reopen");
        let rehydrated = reopened
            .pending_redrives(claim_records_id)
            .expect("records readable");
        assert_eq!(rehydrated.len(), 1);
        assert_eq!(
            rehydrated[0].identity_index,
            Some(IDENTITY_INDEX),
            "the persisted slot must survive a process restart"
        );
        drop(reopened);
        let _ = std::fs::remove_file(&path);
    }

    /// END TO END for #4313 review finding f58ed9d910d8: a resume whose
    /// purge-protection lease is gone must refuse RETRYABLY and must not
    /// re-broadcast.
    ///
    /// A re-broadcast is a chargeable resubmission, and continuing it without a
    /// provable lease is what lets a concurrent clear or wallet removal count
    /// zero live claims and delete the recovery record while the transition is
    /// in flight. The SDK is a bare mock with no expectations registered, so a
    /// re-broadcast would surface as a broadcast-shaped error rather than as
    /// `ShieldedLifecycleBusy`.
    ///
    /// The record must SURVIVE: refusing is only safe because the retry can
    /// still resume it.
    #[tokio::test]
    async fn a_resume_without_a_provable_lease_refuses_instead_of_rebroadcasting() {
        let sdk = Arc::new(dash_sdk::SdkBuilder::new_mock().build().expect("mock sdk"));
        let path = temp_store_path("resume_lease_gate");
        let store = Arc::new(RwLock::new(
            FileBackedShieldedStore::open_path(&path, 100).expect("store opens"),
        ));
        let claim_records_id = SubwalletId::new([0x78; 32], ONE_TIME_CLAIM_RECORDS_ACCOUNT);

        let (_, st_bytes) = stored_claim_transition(&[our_master_key()], DENOMINATION);
        let record = stored_claim_record(st_bytes);
        store
            .write()
            .await
            .arm_redrive(claim_records_id, record.clone())
            .expect("record arms");

        // A token that owns no lease — the state a reaped or displaced claim is
        // left in. Every binding below MATCHES the stored transition, so the
        // mismatch gate cannot be what refuses this.
        let orphaned = crate::wallet::shielded::store::AdmissionToken([0x7F; 16]);
        let outcome = resume_one_time_claim(
            &sdk,
            &store,
            claim_records_id,
            &record,
            orphaned,
            Some(OUR_MASTER_HASH),
            keys_map(&[our_master_key()]),
            DENOMINATION,
            IDENTITY_INDEX,
        )
        .await;

        match outcome {
            OneTimeClaimResume::Resolved(Err(PlatformWalletError::ShieldedLifecycleBusy {
                reason,
            })) => assert!(
                reason.contains("lease"),
                "the refusal must name the lost lease, got: {reason}"
            ),
            other => panic!(
                "a resume without a provable lease must refuse retryably, got {}",
                match other {
                    OneTimeClaimResume::RecordUnusable => "RecordUnusable".to_string(),
                    OneTimeClaimResume::Resolved(r) => format!("Resolved({r:?})"),
                }
            ),
        }

        let surviving = store
            .read()
            .await
            .pending_redrives(claim_records_id)
            .expect("records readable");
        assert_eq!(
            surviving.len(),
            1,
            "refusing is only safe because the record survives for the retry"
        );

        drop(store);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod claim_lease_heartbeat_tests {
    use super::*;
    use crate::wallet::shielded::store::{
        admission_now_ms, AdmissionToken, InMemoryShieldedStore, CLAIM_LEASE_MS,
        CLAIM_LEASE_RENEW_INTERVAL,
    };

    /// How long the initial lease is stamped for in these tests: long enough
    /// that the first heartbeat tick still finds it live (wall-clock time
    /// barely advances under a paused runtime), short enough that the probe
    /// below can tell "renewed" from "not renewed".
    const SHORT_LEASE_MS: u64 = 5_000;

    /// A body shaped like the RESUME path: it does real awaiting and then
    /// returns its own outcome, without ever reaching the fresh-build
    /// broadcast the heartbeat used to wrap.
    async fn resume_shaped_body() -> &'static str {
        for _ in 0..3 {
            tokio::time::sleep(CLAIM_LEASE_RENEW_INTERVAL).await;
        }
        "resumed"
    }

    /// THE BUG (#4313 review finding 8de8d05a): the heartbeat wrapped only the
    /// fresh-build broadcast, so a claim that took the RESUME path — nullifier
    /// queries, repeated identity recovery, re-broadcast, an unbounded
    /// confirmation wait — ran under the initial lease alone. Outrun it and the
    /// lease is reaped, at which point a concurrent purge counts zero live
    /// claims and deletes the record the claim needs.
    ///
    /// Control half: run the same body bare and watch the lease lapse.
    #[tokio::test(start_paused = true)]
    async fn a_resume_shaped_body_run_bare_lets_its_lease_lapse() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x41; 32];
        let token = AdmissionToken([0x41; 16]);
        let t0 = admission_now_ms();
        assert!(store
            .write()
            .await
            .begin_claim_admission(wallet_id, token, t0, SHORT_LEASE_MS)
            .expect("lease"));

        assert_eq!(resume_shaped_body().await, "resumed");

        // Probe: is the lease still live at a point past its ORIGINAL expiry?
        // Nothing re-stamped it, so no.
        assert!(
            !store
                .write()
                .await
                .renew_claim_admission(token, t0 + SHORT_LEASE_MS + 1, CLAIM_LEASE_MS)
                .expect("probe"),
            "without a heartbeat the resume path's lease lapses — this is the bug"
        );
    }

    /// The fix: the heartbeat wraps the COMPLETE admitted claim body, so the
    /// resume path is covered by exactly the same renewal the fresh-build path
    /// gets. Same body, same clock, opposite outcome.
    #[tokio::test(start_paused = true)]
    async fn the_heartbeat_keeps_a_resume_shaped_body_s_lease_live() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x42; 32];
        let token = AdmissionToken([0x42; 16]);
        let t0 = admission_now_ms();
        assert!(store
            .write()
            .await
            .begin_claim_admission(wallet_id, token, t0, SHORT_LEASE_MS)
            .expect("lease"));

        let outcome = under_renewed_claim_lease(&store, token, resume_shaped_body()).await;
        assert_eq!(
            outcome, "resumed",
            "the helper must return the body's value"
        );

        assert!(
            store
                .write()
                .await
                .renew_claim_admission(token, t0 + SHORT_LEASE_MS + 1, CLAIM_LEASE_MS)
                .expect("probe"),
            "the heartbeat must have re-stamped the lease past its original expiry"
        );
    }

    /// The claim-key reservation rides the same token, so the heartbeat holds
    /// the invitation too — a long resume must not lose its claim key to expiry
    /// while its lease is being kept alive.
    #[tokio::test(start_paused = true)]
    async fn the_heartbeat_also_holds_the_claim_key_reservation() {
        use crate::wallet::shielded::store::ClaimKeyReservation;

        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x43; 32];
        let claim_key = [0xC5; 32];
        let claim_records_id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        let holder = AdmissionToken([0x43; 16]);
        let rival = AdmissionToken([0x44; 16]);
        let t0 = admission_now_ms();
        {
            let mut guard = store.write().await;
            assert!(guard
                .begin_claim_admission(wallet_id, holder, t0, SHORT_LEASE_MS)
                .expect("lease"));
            assert_eq!(
                guard
                    .reserve_one_time_claim_key(
                        claim_records_id,
                        claim_key,
                        holder,
                        t0,
                        SHORT_LEASE_MS
                    )
                    .expect("reserve")
                    .reservation,
                ClaimKeyReservation::Acquired
            );
        }

        under_renewed_claim_lease(&store, holder, resume_shaped_body()).await;

        // Past the ORIGINAL reservation expiry, a rival must still be refused.
        let contended = {
            let mut guard = store.write().await;
            assert!(guard
                .begin_claim_admission(wallet_id, rival, t0 + SHORT_LEASE_MS + 1, CLAIM_LEASE_MS)
                .expect("rival lease"));
            guard
                .reserve_one_time_claim_key(
                    claim_records_id,
                    claim_key,
                    rival,
                    t0 + SHORT_LEASE_MS + 1,
                    CLAIM_LEASE_MS,
                )
                .expect("rival reserve")
        };
        assert!(
            !contended.is_acquired(),
            "the heartbeat must carry the claim-key reservation past its original expiry"
        );
    }

    /// A live lease authorizes the chargeable step, and re-stamps itself while
    /// doing so — the broadcast that follows therefore runs at the start of a
    /// full lease window rather than at whatever a long proof build left of one.
    #[tokio::test(start_paused = true)]
    async fn a_live_lease_authorizes_the_chargeable_step_and_is_restamped() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x51; 32];
        let holder = AdmissionToken([0x51; 16]);
        let rival = AdmissionToken([0x52; 16]);
        let t0 = admission_now_ms();
        assert!(store
            .write()
            .await
            .begin_claim_admission(wallet_id, holder, t0, SHORT_LEASE_MS)
            .expect("lease"));

        assert_claim_lease_before_chargeable_step(&store, holder, "the claim broadcast")
            .await
            .expect("a live lease must authorize the broadcast");

        // Re-stamped: past the ORIGINAL expiry the lease is still counted, so a
        // destructive pass still sees a live claim.
        let live = store
            .write()
            .await
            .begin_destructive_admission(
                Some(wallet_id),
                rival,
                t0 + SHORT_LEASE_MS + 1,
                CLAIM_LEASE_MS,
            )
            .expect("destructive admission");
        assert_eq!(
            live, 1,
            "the gate must re-stamp the lease it proves, so the broadcast runs inside a fresh \
             protection window"
        );
    }

    /// THE BUG (#4313 review finding f58ed9d910d8): the heartbeat only LOGGED a
    /// failed renewal and let the claim run on. Once the lease is gone,
    /// `renew_claim_admission` refuses to resurrect it and destructive
    /// admission reaps it before counting live claims — so a concurrent clear
    /// or wallet removal counts zero claims and deletes the pending row while
    /// the transition is on the wire. For a padded single-note bundle that row
    /// is the only handle to the randomized identity id.
    ///
    /// So a chargeable step must refuse, retryably, when the lease is gone.
    #[tokio::test(start_paused = true)]
    async fn a_lapsed_lease_refuses_the_chargeable_step() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x53; 32];
        let holder = AdmissionToken([0x53; 16]);
        let t0 = admission_now_ms();
        assert!(store
            .write()
            .await
            .begin_claim_admission(wallet_id, holder, t0, SHORT_LEASE_MS)
            .expect("lease"));

        // The lease is displaced exactly as a purge's reap would displace it.
        store
            .write()
            .await
            .end_claim_admission(holder)
            .expect("release the lease");

        let refused =
            assert_claim_lease_before_chargeable_step(&store, holder, "the claim broadcast").await;
        assert!(
            matches!(
                refused,
                Err(PlatformWalletError::ShieldedLifecycleBusy { .. })
            ),
            "a chargeable step must refuse — RETRYABLY — without a lease it can prove; got \
             {refused:?}"
        );
    }

    /// The same refusal for a lease that merely EXPIRED on the wall clock,
    /// which is the case a forward clock adjustment or a long executor
    /// suspension produces: `renew_claim_admission` will not resurrect it, so
    /// ownership can no longer be proven and the claim must not broadcast.
    #[tokio::test]
    async fn an_expired_lease_cannot_be_resurrected_to_authorize_a_broadcast() {
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let wallet_id: WalletId = [0x54; 32];
        let holder = AdmissionToken([0x54; 16]);
        let t0 = admission_now_ms();
        // A zero-length lease: `expires_at == t0`, already expired against any
        // `now >= t0` the gate reads. Expressing the expiry through the STAMP
        // rather than by advancing time is what makes this deterministic —
        // `admission_now_ms` is wall-clock, which a paused runtime never moves,
        // so a short-but-nonzero lease races the gate.
        assert!(store
            .write()
            .await
            .begin_claim_admission(wallet_id, holder, t0, 0)
            .expect("lease"));

        // `renew_claim_admission`'s `expires_at > now` predicate deliberately
        // refuses to resurrect it — the store-level behaviour this gate is
        // built on, and the reason a lapsed claim cannot renew its way back
        // into ownership.
        let renewed = store
            .write()
            .await
            .renew_claim_admission(holder, admission_now_ms(), CLAIM_LEASE_MS)
            .expect("renew must not error");
        assert!(
            !renewed,
            "precondition: an expired lease is deliberately not resurrectable"
        );

        let refused =
            assert_claim_lease_before_chargeable_step(&store, holder, "the claim re-broadcast")
                .await;
        assert!(
            matches!(
                refused,
                Err(PlatformWalletError::ShieldedLifecycleBusy { .. })
            ),
            "an expired lease must not authorize a re-broadcast; got {refused:?}"
        );
    }
}

/// The claim's removal fence (#4313 review finding 4a2c679745bb).
#[cfg(test)]
mod claim_removal_fence_tests {
    use super::*;
    use crate::wallet::shielded::keys::OrchardKeySet;
    use crate::wallet::shielded::store::{
        admission_now_ms, AdmissionToken, InMemoryShieldedStore, DESTRUCTIVE_BARRIER_MS,
    };
    use dpp::identity::identity_public_key::v0::IdentityPublicKeyV0;
    use dpp::identity::{KeyType, Purpose, SecurityLevel};
    use dpp::platform_value::BinaryData;
    use std::sync::atomic::AtomicBool;

    /// Smallest member of the versioned exit-denomination set (0.1 DASH).
    const DENOMINATION: u64 = 10_000_000_000;

    /// A prover that cannot prove. Reaching it means the claim got past the
    /// removal fence and started building a bundle, which is the whole failure
    /// this test exists to catch — so it is `unreachable!`, not a stub.
    #[derive(Debug)]
    struct NeverProver;

    impl OrchardProver for NeverProver {
        fn proving_key(&self) -> &grovedb_commitment_tree::ProvingKey {
            unreachable!("a claim refused at the removal fence must never build a bundle")
        }
    }

    /// A signer that cannot sign, for the same reason.
    #[derive(Debug)]
    struct NeverSigner;

    #[async_trait::async_trait]
    impl Signer<IdentityPublicKey> for NeverSigner {
        async fn sign(
            &self,
            _key: &IdentityPublicKey,
            _data: &[u8],
        ) -> Result<BinaryData, dpp::ProtocolError> {
            unreachable!("a claim refused at the removal fence must never sign")
        }

        async fn sign_create_witness(
            &self,
            _key: &IdentityPublicKey,
            _data: &[u8],
        ) -> Result<dpp::address_funds::AddressWitness, dpp::ProtocolError> {
            unreachable!("a claim refused at the removal fence must never sign")
        }

        fn can_sign_with(&self, _key: &IdentityPublicKey) -> bool {
            true
        }
    }

    fn master_auth_key() -> (IdentityPublicKey, IdentityPublicKeyInCreation) {
        let key = IdentityPublicKey::V0(IdentityPublicKeyV0 {
            id: 0,
            purpose: Purpose::AUTHENTICATION,
            security_level: SecurityLevel::MASTER,
            contract_bounds: None,
            key_type: KeyType::ECDSA_HASH160,
            read_only: false,
            data: BinaryData::new(vec![0xA1; 20]),
            disabled_at: None,
        });
        let in_creation: IdentityPublicKeyInCreation = (&key).into();
        (key, in_creation)
    }

    /// The claimer's own default Orchard address, for the change note.
    fn change_address() -> OrchardAddress {
        let keys = OrchardKeySet::from_seed(&[0x42u8; 64], dashcore::Network::Testnet, 0)
            .expect("derive the claimer's key set");
        OrchardAddress::from_raw_bytes(&keys.default_address.to_raw_address_bytes())
            .expect("the derived default address is a valid Orchard address")
    }

    /// A claim that begins AFTER its wallet's removal has committed must be
    /// refused before it scans, builds, or broadcasts anything
    /// (#4313 review finding 4a2c679745bb).
    ///
    /// The reviewer's sequence: an FFI caller resolves and retains the wallet
    /// and coordinator, `unregister_wallet_with` then commits the detach,
    /// purges the store, RELEASES its destructive barrier and drops the wallet
    /// from the manager — and only then does the stale handle start executing.
    /// With the barrier already released, the claim's own admission succeeds,
    /// so admission alone does not refuse it; before the fix it went on to arm
    /// a pending row and broadcast for a wallet the host had already removed,
    /// and its registration tail — finding no manager entry — left that row
    /// behind.
    ///
    /// This drives that state directly: `detached` is already `true` and no
    /// destructive admission is held, exactly as after a completed removal.
    #[tokio::test]
    async fn a_claim_that_starts_after_a_committed_removal_is_refused() {
        let sdk = Arc::new(dash_sdk::Sdk::new_mock());
        let store = Arc::new(RwLock::new(InMemoryShieldedStore::new()));
        let guards = ForeignClaimGuards::default();
        let checkpoints = super::super::sync::ForeignScanCheckpointCache::default();
        let wallet_id: WalletId = [0x11; 32];

        // The removal has fully committed AND released its barrier.
        let detached = AtomicBool::new(true);

        let (_key, _in_creation) = master_auth_key();
        let result = identity_create_from_one_time_key(
            &sdk,
            &store,
            &guards,
            &checkpoints,
            &detached,
            wallet_id,
            zeroize::Zeroizing::new([0x01u8; 32]),
            None,
            &change_address(),
            3,
            vec![master_auth_key()],
            DENOMINATION,
            PlatformAddress::P2pkh([0x07; 20]),
            &NeverSigner,
            &NeverProver,
        )
        .await;

        // RED before the fix: the claim ran on, and the `unreachable!` prover /
        // signer above (or a mock-SDK scan error) stood in for the broadcast it
        // would have made for a removed wallet.
        match result {
            Err(PlatformWalletError::WalletNotFound(reason)) => {
                assert!(
                    reason.contains(&hex::encode(wallet_id)),
                    "the refusal must name the removed wallet, got: {reason}"
                );
            }
            other => panic!(
                "a claim beginning after a committed removal must be refused as WalletNotFound, \
                 got {other:?}"
            ),
        }

        // Nothing was armed for the removed wallet — the point of refusing
        // under admission rather than after the broadcast.
        let record_id = SubwalletId::new(wallet_id, ONE_TIME_CLAIM_RECORDS_ACCOUNT);
        assert!(
            store
                .read()
                .await
                .pending_redrives(record_id)
                .expect("records readable")
                .is_empty(),
            "a refused claim must leave no pending record behind"
        );

        // And the admission the refusal took was RELEASED, not left to expire:
        // a destructive operation on this wallet must see zero live claims.
        // The early return is on the fence path, so this is the only test that
        // exercises that path's release.
        let token = AdmissionToken::generate().expect("token");
        let live = store
            .write()
            .await
            .begin_destructive_admission(
                Some(wallet_id),
                token,
                admission_now_ms(),
                DESTRUCTIVE_BARRIER_MS,
            )
            .expect("admission");
        assert_eq!(
            live, 0,
            "the refusal must release its claim lease before returning"
        );
    }
}
