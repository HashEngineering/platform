//! Storage abstraction for shielded wallet state.
//!
//! The `ShieldedStore` trait decouples `ShieldedWallet` from any
//! particular persistence backend. Consumers provide their own
//! implementation (e.g. SwiftData via the host persister) while
//! tests can use [`InMemoryShieldedStore`].
//!
//! # Multi-tenant scoping
//!
//! Decrypted notes, nullifier bookkeeping, and per-account sync
//! watermarks are scoped by [`SubwalletId`] (a `(wallet_id,
//! account_index)` tuple) so a single store can host every wallet
//! and every shielded account on the same network. The Orchard
//! commitment tree itself is **not** scoped — the on-chain
//! commitment stream is identical for every consumer on a given
//! network, so one tree backs them all.
//!
//! # Note format
//!
//! `ShieldedNote::note_data` is a serialized `orchard::Note` (115
//! bytes). The witness path returned by [`ShieldedStore::witness`]
//! is the typed `grovedb_commitment_tree::MerklePath` because that
//! type doesn't implement serde — a bytes contract would force
//! every caller through a serializer that doesn't exist.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt;

use super::balance::ShieldedBalanceSource;
use crate::wallet::platform_wallet::WalletId;

/// Identifies a single shielded "subwallet" — one Orchard account
/// within one wallet. Used to scope notes, nullifier indices, and
/// sync watermarks inside a [`ShieldedStore`] so a single store
/// can hold state for many wallets/accounts without leakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubwalletId {
    /// 32-byte wallet identifier (matches `PlatformWallet::wallet_id`).
    pub wallet_id: [u8; 32],
    /// ZIP-32 account index (`m / 32' / coin_type' / account'`).
    pub account_index: u32,
}

impl SubwalletId {
    /// Construct a [`SubwalletId`] from its parts.
    pub fn new(wallet_id: [u8; 32], account_index: u32) -> Self {
        Self {
            wallet_id,
            account_index,
        }
    }
}

/// A note decrypted and owned by a specific subwallet.
///
/// Carries the bookkeeping the spend pipeline needs without
/// pulling the orchard crate into this trait. The actual
/// `orchard::Note` is in `note_data` as 115 bytes
/// (`recipient(43) || value(8 LE) || rho(32) || rseed(32)`).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShieldedNote {
    /// Global position in the commitment tree.
    pub position: u64,
    /// Extracted note commitment (32 bytes).
    pub cmx: [u8; 32],
    /// Nullifier for detecting when spent (32 bytes).
    pub nullifier: [u8; 32],
    /// Proven platform height of the chunk fetch that surfaced the note.
    /// Stamped per-batch — the SAME height OVK-recovered outgoing notes
    /// from that chunk get — so the activity deriver can cluster one
    /// bundle's incoming change and outgoing send together by height.
    ///
    /// This is an *observed at-or-before* bound (the tip the fetch was
    /// proven at), NOT the note's inclusion height, which the proof
    /// doesn't carry. It is a grouping key only — never surface it as
    /// when the note was mined (scan-derived activity entries carry
    /// `block_height: None` for exactly this reason).
    pub block_height: u64,
    /// Whether the nullifier was seen on-chain (spent).
    pub is_spent: bool,
    /// Note value in credits.
    pub value: u64,
    /// Serialized `orchard::Note` bytes (115 bytes).
    pub note_data: Vec<u8>,
}

/// A note this subwallet **sent**, recovered during the note scan via
/// the wallet's Outgoing Viewing Key (the Zcash outgoing-transaction-
/// history mechanism).
///
/// Unlike [`ShieldedNote`] (which is a note the wallet *received* and
/// can later spend), this is a minimal record of an *outgoing*
/// payment — who the wallet paid, how much, and with what memo —
/// kept purely for send-history display. It carries no spend
/// bookkeeping (no nullifier / position / witness) because the wallet
/// cannot spend a note it sent to someone else.
///
/// Keyed by `cmx` (the recovered output note's commitment), which is
/// globally unique on-chain, so recording the same recovered note
/// twice (a re-scan of the same chunk) is idempotent.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShieldedOutgoingNote {
    /// Extracted note commitment (32 bytes) of the note that was sent.
    /// Primary key — unique per on-chain output.
    pub cmx: [u8; 32],
    /// Recipient's raw Orchard address (43 bytes,
    /// `Address::to_raw_address_bytes`). The wallet paid this address.
    /// Stored as a `Vec` rather than `[u8; 43]` because `serde`'s derive
    /// only covers fixed arrays up to length 32; always 43 bytes for a
    /// recovered note.
    pub recipient: Vec<u8>,
    /// Value sent, in credits.
    pub value: u64,
    /// Raw Dash memo bytes (`DashMemo`, 36 bytes). Stored as a `Vec`
    /// rather than `[u8; 36]` so the persisted shape stays flexible if
    /// the memo size ever changes; always 36 bytes for a recovered note.
    pub memo: Vec<u8>,
    /// Proven platform height of the chunk fetch that recovered the
    /// note — the same per-batch *observed at-or-before* bound (and
    /// grouping key) as [`ShieldedNote::block_height`]; NOT the height
    /// the send was mined at.
    pub block_height: u64,
}

/// A pending reservation that carries a recorded anchor:
/// `(nullifier, anchor, activity_id, anchor_height)`, `anchor_height` being a
/// proved height at which `anchor` was in Platform's recorded set, when known.
/// Yielded by [`ShieldedStore::stale_pending_spends`] for the sync
/// reconcile, which releases the reservation once `anchor` is proven pruned
/// (the spend can then never execute).
pub type StalePendingSpend = ([u8; 32], [u8; 32], Option<[u8; 32]>, Option<u64>);

/// An unresolved signed transition and its durable retry guard: the signed
/// transition bytes plus everything the sync-time re-drive needs to
/// resolve the ambiguity actively — re-broadcast the transition
/// ([`nullifiers`](Self::nullifiers) detect a landing, `anchor` feeds
/// the prune backstop, `activity_id` links the UI row, `attempts`
/// bounds the retries.
///
/// Identity-funded shields arm this record before broadcast and have no input
/// nullifiers. Their guard survives even after rebroadcasting stops. Note spends
/// arm it before their first dispatch too, and keep it unless the transition
/// never left the wallet ([`ShieldedStore::retire_undelivered_spend`]): a
/// spend that may have reached a node may or may not have executed, and a
/// process that exits during its broadcast or its result wait leaves the
/// record to restore its notes' reservation from. Re-broadcasting the
/// byte-identical transition is fund-safe (identical nullifiers cannot be
/// spent twice) and converts silence into a confirmation (next scan sees the
/// nullifiers spent); a spend that never lands keeps its notes held until its
/// anchor is proven pruned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRedrive {
    /// Activity-entry id of the spend (sha256 of visible output cmxs);
    /// the spend-level key — every nullifier of one spend shares it.
    pub activity_id: [u8; 32],
    /// Platform-recorded anchor the spend was built against.
    pub anchor: [u8; 32],
    /// Nullifiers of every note the spend consumes.
    pub nullifiers: Vec<[u8; 32]>,
    /// The signed state transition, platform-serialized byte-exact as
    /// originally broadcast.
    pub st_bytes: Vec<u8>,
    /// Re-broadcast attempts made so far.
    pub attempts: u32,
    /// A proved height at which [`Self::anchor`] was in Platform's recorded
    /// anchor set: the height of the set the spend's anchor was read from, or
    /// of a later set seen holding it
    /// ([`ShieldedStore::note_anchor_recorded`]). A set proved at or above it
    /// that lacks the anchor shows it pruned rather than newer. `None` for a
    /// record saved without it, by a build before heights were kept: its
    /// anchor is proven pruned only once a set is seen holding it, or by a
    /// proved pool count reaching the store's bound on the trees its anchors
    /// were taken from ([`ShieldedStore::legacy_anchor_tree_bound`]).
    pub anchor_height: Option<u64>,
    /// A proven identity nonce has made this exact debit impossible to execute
    /// again. Stop broadcasting, but keep its unresolved-payment guard until
    /// the scan confirms the outputs. Always false for note-spend redrives.
    pub identity_nonce_finalized: bool,
    /// The host explicitly accepted that this payment may have executed or may
    /// still execute. Retain the signed record for reconciliation/audit, but stop
    /// automatic retries and permit separately authorized new payments.
    /// This is never a proof of failure and is always false for note spends.
    pub identity_user_abandoned: bool,
    /// For a one-time-key CLAIM record: the local DIP-9 identity slot the
    /// original attempt was creating the identity at. `None` for every
    /// ordinary spend redrive (which registers no identity), and for claim
    /// records written before this field existed.
    ///
    /// Persisted rather than derived because it appears NOWHERE in
    /// `st_bytes` — it is a purely local placement, so the transition cannot
    /// witness it and a resume has nothing else to check a retry's slot
    /// against. Without it a retry could present the original keys with a
    /// different index and `IdentityManager::add_identity` would insert into
    /// an occupied slot without complaint (#4313 review finding 5d4d6efa).
    pub identity_index: Option<u32>,
}

/// The DashPay private contact request a saved spend pays for: which identity
/// sends it to which, and its account reference. Kept with the spend's
/// record, so a restart restores the pair's reservation from the record
/// without the keys that open its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateContactPayment {
    /// The identity sending the request.
    pub sender: [u8; 32],
    /// The identity it is to.
    pub recipient: [u8; 32],
    /// The request's account reference, whose version tells a rotation from
    /// the request it rotates.
    pub account_reference: u32,
}

impl PrivateContactPayment {
    /// The bytes the file store keeps it as: sender, recipient, then the
    /// account reference little-endian.
    pub(super) fn to_bytes(self) -> [u8; 68] {
        let mut bytes = [0u8; 68];
        bytes[..32].copy_from_slice(&self.sender);
        bytes[32..64].copy_from_slice(&self.recipient);
        bytes[64..].copy_from_slice(&self.account_reference.to_le_bytes());
        bytes
    }

    /// The payment [`Self::to_bytes`] wrote, or `None` for other bytes.
    pub(super) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 68 {
            return None;
        }
        Some(Self {
            sender: bytes.get(..32)?.try_into().ok()?,
            recipient: bytes.get(32..64)?.try_into().ok()?,
            account_reference: u32::from_le_bytes(bytes.get(64..)?.try_into().ok()?),
        })
    }
}

/// One private contact request payment the wallet may have broadcast, as the
/// shielded store's private payment journal keeps it: the request it pays
/// for, the commitment of the entry it appends and the nullifiers its spend
/// consumes. Saved durably before the payment can reach the network
/// ([`ShieldedStore::arm_private_contact_payment`]) and kept until the
/// payment provably resolved ([`ShieldedStore::discharge_private_payment`]),
/// whatever happens to the spend's notes and redrive record: the pair it
/// holds is restored from it after a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivatePaymentRecord {
    /// The account whose notes pay for it.
    pub account_index: u32,
    /// The spend's activity id, which keys its redrive record.
    pub activity_id: [u8; 32],
    /// The recorded anchor the spend was built against: once Platform no
    /// longer records it, the spend can never execute, which with an input
    /// note still unspent and the entry absent from the store proves the
    /// payment expired.
    pub anchor: [u8; 32],
    /// The proved height of the recorded-anchor set `anchor` was chosen from:
    /// the anchor was recorded then, so only its absence from a set proved at
    /// or above this height shows it pruned. `None` when not known, as for a
    /// payment saved before the journal or the redrive record kept it, whose
    /// expiry is then proven only by a proved pool count reaching the store's
    /// bound on the trees its anchors were taken from
    /// ([`ShieldedStore::legacy_anchor_tree_bound`]). Kept as `u64::MAX` in
    /// the record's bytes ([`Self::to_bytes`]).
    pub anchor_height: Option<u64>,
    /// The commitment of the entry it appends, which keys the record.
    pub commitment: [u8; 32],
    /// The request it pays for.
    pub payment: PrivateContactPayment,
    /// The nullifiers of the notes its spend consumes; never empty.
    pub nullifiers: Vec<[u8; 32]>,
    /// SHA-256 of the whole entry it appends: the expiry's read of the store
    /// takes the payment as executed only for an entry that hashes to it, not
    /// for any entry that merely starts with [`Self::commitment`], which
    /// anyone who saw the commitment can append. `None` for a record saved
    /// before it was kept (format version 3), which matches on the commitment.
    pub entry_hash: Option<[u8; 32]>,
    /// The sender identity's private store read position when the payment was
    /// made: its entry is appended at or after it, so the expiry's read of the
    /// store starts there. `None` for a record saved before it was kept, or
    /// with no position known, which is read from the store's start.
    pub store_position: Option<u64>,
}

/// An entry a private contact request payment appends, as its journal record
/// keeps it ([`ShieldedStore::arm_private_contact_payment`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivatePaymentEntry {
    /// The entry's first 32 bytes, which key its record.
    pub commitment: [u8; 32],
    /// SHA-256 of the whole entry ([`PrivatePaymentRecord::entry_hash`]).
    pub entry_hash: [u8; 32],
}

impl PrivatePaymentEntry {
    /// An entry of `commitment` with a placeholder hash, for tests whose
    /// expiry never reads the store.
    #[cfg(test)]
    pub(crate) fn for_commitment(commitment: [u8; 32]) -> Self {
        Self {
            commitment,
            entry_hash: [0; 32],
        }
    }
}

impl PrivatePaymentRecord {
    /// How an unknown [`Self::anchor_height`] is kept in the record's bytes:
    /// no proved height reaches it.
    const UNKNOWN_ANCHOR_HEIGHT: u64 = u64::MAX;

    /// The format version [`Self::to_bytes`] writes. Versions 1 and 2, written
    /// before the anchor and its height were kept, do not read back: their
    /// wallet's holds stay. Version 3, written before the entry's hash and the
    /// store position were kept, reads back with neither.
    const FORMAT_VERSION: u8 = 4;
    /// The version before [`Self::FORMAT_VERSION`], still read.
    const FORMAT_VERSION_WITHOUT_ENTRY_HASH: u8 = 3;
    /// The bytes before the nullifiers: version, account index, activity id,
    /// anchor, anchor height, commitment, sender, recipient, account reference
    /// and nullifier count.
    const HEADER_LEN: usize = 1 + 4 + 32 + 32 + 8 + 32 + 32 + 32 + 4 + 2;

    /// The bytes the file store keeps it as: [`Self::FORMAT_VERSION`], the
    /// account index little-endian, the activity id, the anchor, the anchor
    /// height little-endian, the commitment, the sender, the recipient, the
    /// account reference little-endian, the nullifier count as a
    /// little-endian `u16`, the nullifiers, then the entry's hash and the
    /// store position, each a presence byte (0 or 1) and, when present, the
    /// 32 bytes or the position little-endian. `None` when it has no
    /// nullifier or more than a `u16` counts.
    pub(super) fn to_bytes(&self) -> Option<Vec<u8>> {
        let count = u16::try_from(self.nullifiers.len()).ok()?;
        if count == 0 {
            return None;
        }
        let mut bytes = Vec::with_capacity(Self::HEADER_LEN + 32 * self.nullifiers.len());
        bytes.push(Self::FORMAT_VERSION);
        bytes.extend_from_slice(&self.account_index.to_le_bytes());
        bytes.extend_from_slice(&self.activity_id);
        bytes.extend_from_slice(&self.anchor);
        bytes.extend_from_slice(
            &self
                .anchor_height
                .unwrap_or(Self::UNKNOWN_ANCHOR_HEIGHT)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&self.commitment);
        bytes.extend_from_slice(&self.payment.sender);
        bytes.extend_from_slice(&self.payment.recipient);
        bytes.extend_from_slice(&self.payment.account_reference.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        for nullifier in &self.nullifiers {
            bytes.extend_from_slice(nullifier);
        }
        match &self.entry_hash {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(hash);
            }
            None => bytes.push(0),
        }
        match self.store_position {
            Some(position) => {
                bytes.push(1);
                bytes.extend_from_slice(&position.to_le_bytes());
            }
            None => bytes.push(0),
        }
        Some(bytes)
    }

    /// The record [`Self::to_bytes`] wrote, or one of version 3, or `None`
    /// for any other bytes: another version, a length that does not match the
    /// count, or no nullifier.
    pub(super) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (&version, rest) = bytes.split_first()?;
        if version != Self::FORMAT_VERSION && version != Self::FORMAT_VERSION_WITHOUT_ENTRY_HASH {
            return None;
        }
        let (account_index, rest) = rest.split_first_chunk::<4>()?;
        let (activity_id, rest) = rest.split_first_chunk::<32>()?;
        let (anchor, rest) = rest.split_first_chunk::<32>()?;
        let (anchor_height, rest) = rest.split_first_chunk::<8>()?;
        let (commitment, rest) = rest.split_first_chunk::<32>()?;
        let (sender, rest) = rest.split_first_chunk::<32>()?;
        let (recipient, rest) = rest.split_first_chunk::<32>()?;
        let (account_reference, rest) = rest.split_first_chunk::<4>()?;
        let (count, rest) = rest.split_first_chunk::<2>()?;
        let count = usize::from(u16::from_le_bytes(*count));
        let (nullifiers, rest) = rest.split_at_checked(32 * count)?;
        let (nullifiers, remainder) = nullifiers.as_chunks::<32>();
        if count == 0 || nullifiers.len() != count || !remainder.is_empty() {
            return None;
        }
        let (entry_hash, store_position) = if version == Self::FORMAT_VERSION {
            let (&has_hash, rest) = rest.split_first()?;
            let (entry_hash, rest) = match has_hash {
                0 => (None, rest),
                1 => {
                    let (hash, rest) = rest.split_first_chunk::<32>()?;
                    (Some(*hash), rest)
                }
                _ => return None,
            };
            let (&has_position, rest) = rest.split_first()?;
            let (store_position, rest) = match has_position {
                0 => (None, rest),
                1 => {
                    let (position, rest) = rest.split_first_chunk::<8>()?;
                    (Some(u64::from_le_bytes(*position)), rest)
                }
                _ => return None,
            };
            if !rest.is_empty() {
                return None;
            }
            (entry_hash, store_position)
        } else {
            if !rest.is_empty() {
                return None;
            }
            (None, None)
        };
        Some(Self {
            account_index: u32::from_le_bytes(*account_index),
            activity_id: *activity_id,
            anchor: *anchor,
            anchor_height: Some(u64::from_le_bytes(*anchor_height))
                .filter(|height| *height != Self::UNKNOWN_ANCHOR_HEIGHT),
            commitment: *commitment,
            payment: PrivateContactPayment {
                sender: *sender,
                recipient: *recipient,
                account_reference: u32::from_le_bytes(*account_reference),
            },
            nullifiers: nullifiers.to_vec(),
            entry_hash,
            store_position,
        })
    }
}

/// What the wallet knows about an identity debit's local recovery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityDebitRecoveryStatus {
    /// The original signed transaction may still be retried.
    Retrying,
    /// Its nonce is unavailable; output confirmation is still missing.
    Parked,
    /// The user accepted an unknown outcome and stopped automatic recovery.
    Unknown,
}

/// An active or explicitly abandoned identity debit retained by the wallet.
/// Missing decoded fields indicate a damaged record; the wallet/account/activity
/// key still allows a deliberate, precisely scoped recovery decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityDebitRecoveryRecord {
    pub account_index: u32,
    pub activity_id: [u8; 32],
    pub identity_id: Option<[u8; 32]>,
    pub nonce: Option<u64>,
    pub amount: Option<u64>,
    pub status: IdentityDebitRecoveryStatus,
}

/// The result of [`SubwalletState::mark_spent`].
///
/// `newly_spent` preserves the historical `bool` return (the
/// unspent→spent transition, which the durable store keys its
/// note-row write on). `dropped_redrives` carries the activity ids of
/// any redrive records resolved by this nullifier so the durable store
/// can mirror the SQLite deletion — even on the already-spent path,
/// where the in-memory drop happens but `newly_spent` is false.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct MarkSpentOutcome {
    pub newly_spent: bool,
    pub dropped_redrives: Vec<[u8; 32]>,
}

/// Storage abstraction for shielded wallet state.
///
/// Consumers implement this for their persistence layer. The
/// trait is object-safe (no generics on method signatures) so it
/// can be stored behind `Arc<RwLock<dyn ShieldedStore>>`.
///
/// All mutating methods take `&mut self` so implementations can
/// batch writes without interior mutability.
///
/// A store is `'static`: a spend dropped before it hands its notes over
/// releases their reservation on a spawned task when the store is locked.
pub trait ShieldedStore: Send + Sync + 'static {
    /// The error type returned by storage operations.
    type Error: StdError + Send + Sync + 'static;

    // ── Notes (per-subwallet) ──────────────────────────────────────────

    /// Persist a newly decrypted note for `id`.
    fn save_note(&mut self, id: SubwalletId, note: &ShieldedNote) -> Result<(), Self::Error>;

    /// Return all unspent notes for `id`.
    fn get_unspent_notes(&self, id: SubwalletId) -> Result<Vec<ShieldedNote>, Self::Error>;

    /// The same reservation-aware sum used by sync and local balance reads.
    /// Implementations must reject sums exceeding `u64::MAX` with their storage
    /// error, rather than wrapping, saturating, or panicking.
    fn spendable_balance(&self, id: SubwalletId) -> Result<u64, Self::Error>;

    /// Return all notes (spent and unspent) for `id`.
    fn get_all_notes(&self, id: SubwalletId) -> Result<Vec<ShieldedNote>, Self::Error>;

    /// Mark `id`'s note with `nullifier` as spent. Returns `true`
    /// if a matching unspent note was found.
    fn mark_spent(&mut self, id: SubwalletId, nullifier: &[u8; 32]) -> Result<bool, Self::Error>;

    /// Reserve `id`'s note with `nullifier` against an in-flight
    /// spend so concurrent callers can't pick the same note.
    /// Returns `true` if the nullifier was newly added to the
    /// pending set, `false` if it was already pending.
    ///
    /// The reservation itself is held in memory. A spend saves a redrive
    /// record before its first dispatch ([`Self::arm_redrive`]), which
    /// carries its nullifiers: a store that persists records restores the
    /// reservation from it on reopen, so a process that exits during a
    /// broadcast keeps the notes reserved. A reservation with no saved
    /// record (its spend exited before saving one, so before anything was
    /// dispatched) does not survive a restart.
    ///
    /// `unspent_notes` skips notes whose nullifier is in the
    /// pending set, so a successful `mark_pending` immediately
    /// removes the note from selection candidates.
    fn mark_pending(&mut self, id: SubwalletId, nullifier: &[u8; 32]) -> Result<bool, Self::Error>;

    /// Release the reservation taken by [`Self::mark_pending`].
    /// Returns `true` if the nullifier was actually pending and
    /// got removed; `false` is a no-op (paired clear from the
    /// rollback path on a transition that never marked pending,
    /// or a stale clear after the spend already promoted to
    /// `mark_spent`). Clears whoever holds it: for a resolution of the
    /// spend (a pruned anchor). A spend attempt releases its own with
    /// [`Self::clear_pending_reserved_by`].
    fn clear_pending(&mut self, id: SubwalletId, nullifier: &[u8; 32])
        -> Result<bool, Self::Error>;

    /// [`Self::mark_pending`] for the spend attempt `reservation`, which owns
    /// the reservation: only [`Self::clear_pending_reserved_by`] with the
    /// same attempt, or a resolution of the spend, clears it.
    fn mark_pending_by(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        reservation: u64,
    ) -> Result<bool, Self::Error>;

    /// Release `id`'s reservation of `nullifier`, and the redrive records
    /// carrying it, only while the spend attempt `reservation` still holds
    /// it ([`Self::mark_pending_by`]), compared and cleared in one call
    /// under the store lock: a stale attempt's cleanup never releases a
    /// newer attempt's notes or deletes its records. Returns whether it was
    /// released.
    fn clear_pending_reserved_by(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        reservation: u64,
    ) -> Result<bool, Self::Error>;

    /// Attach the recorded `anchor` the spend was built against, the proved
    /// height of the anchor set it was read from (`anchor_height`), and the
    /// linked `activity_id`, to `id`'s pending reservation for `nullifier`
    /// (taken earlier by [`Self::mark_pending`]). No-op if the nullifier is
    /// no longer pending. The sync reconcile uses the stored anchor and its
    /// height to detect a stranded spend whose anchor Platform has pruned
    /// (see [`Self::stale_pending_spends`]).
    fn set_pending_spend(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        anchor: [u8; 32],
        activity_id: [u8; 32],
        anchor_height: Option<u64>,
    ) -> Result<(), Self::Error>;

    /// Note that `anchor` was in Platform's recorded set proved at `height`:
    /// each of `id`'s reservations built on it and each of its records that
    /// carry no anchor height get `height`, a record's durably. A set proved
    /// at or above it that lacks the anchor then shows it pruned. One that
    /// already carries a height keeps it.
    fn note_anchor_recorded(
        &mut self,
        id: SubwalletId,
        anchor: &[u8; 32],
        height: u64,
    ) -> Result<(), Self::Error>;

    /// An upper bound on the size of the commitment tree any record this store
    /// loaded at open without an anchor height took its anchor from: the
    /// largest tree the store has held, kept durably across resets. Such a
    /// record was saved by an earlier process, whose spends were built on
    /// roots of its local tree, which only holds proved commitments. So once
    /// a proved pool count at or below an anchor set's height reaches this
    /// bound, the anchor was recorded by then, and the set's lack of it shows
    /// it pruned rather than newer (`PrunedAnchorEvidence::proves_pruned`).
    /// `None` when the store keeps no such bound: such records stay held.
    fn legacy_anchor_tree_bound(&self) -> Option<u64> {
        None
    }

    /// Pending reservations for `id` that carry a recorded anchor —
    /// `(nullifier, anchor, activity_id, anchor_height)` per
    /// built-but-unconfirmed spend. Empty when `id` has no anchored reservations (the common
    /// case, which lets the sync reconcile skip its network round-trip).
    /// The reconcile checks each anchor against Platform's recorded set
    /// and releases the reservation via [`Self::clear_pending`] when the
    /// anchor is pruned — the spend can then never execute.
    fn stale_pending_spends(&self, id: SubwalletId) -> Result<Vec<StalePendingSpend>, Self::Error>;

    // ── Re-drivable unconfirmed spends (per-subwallet) ─────────────────

    /// Persist the re-drivable record of a spend, saved before its first
    /// dispatch. Keyed by `redrive.activity_id`;
    /// re-arming the same id overwrites. Unlike the bare `mark_pending`
    /// reservations, redrive records survive a restart where the backend
    /// persists them (the file store does): on reopen both the record
    /// and its note reservations are rehydrated, so the re-drive — and
    /// the linked activity row's eventual Confirmed/Failed flip —
    /// continue across relaunches.
    fn arm_redrive(&mut self, id: SubwalletId, redrive: PendingRedrive) -> Result<(), Self::Error>;

    /// [`Self::arm_redrive`] for a spend paying for the DashPay private
    /// contact request `payment`, which is kept with the record and dropped
    /// with it, and the private payment journal's record of each entry the
    /// spend appends (`entries`, each with the hash of the whole entry),
    /// which outlives it ([`PrivatePaymentRecord`]), with the sender's private
    /// store read position when it paid (`store_position`). All of it is saved
    /// in one write that is durable once this returns, across a crash or power
    /// loss too: the spend is broadcast only after it. A store that cannot
    /// keep all of it durably fails, and the payment is refused.
    /// `anchor_height` is the proved height of the recorded-anchor set the
    /// spend's anchor was chosen from ([`PrivatePaymentRecord::anchor_height`]).
    fn arm_private_contact_payment(
        &mut self,
        id: SubwalletId,
        redrive: PendingRedrive,
        payment: PrivateContactPayment,
        entries: &[PrivatePaymentEntry],
        anchor_height: u64,
        store_position: Option<u64>,
    ) -> Result<(), Self::Error>;

    /// The armed records of `wallet_id` that pay for a private contact
    /// request, each with the request ([`Self::arm_private_contact_payment`]).
    /// Fails when one of them carries an association that does not read.
    fn private_contact_redrives_for_wallet(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<(PendingRedrive, PrivateContactPayment)>, Self::Error>;

    // ── Private payment journal (per-wallet) ───────────────────────────

    /// Whether `wallet_id`'s private payment journal is open
    /// ([`Self::open_private_payment_journal`]): then it is authoritative for
    /// the wallet's private payments, so one marked unresolved with no record
    /// in it has resolved or was never broadcast. A journal not open yet, as
    /// in a store that was deleted or replaced, cannot tell.
    fn private_payment_journal_open(&self, wallet_id: WalletId) -> Result<bool, Self::Error>;

    /// Open `wallet_id`'s private payment journal, durably: called once a
    /// restoration of the wallet's private payments from it succeeded, before
    /// the wallet can pay. Saving a journal record opens it too.
    fn open_private_payment_journal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error>;

    /// Every journal record of `wallet_id`'s private payments. Fails when one
    /// of the wallet's records, or one whose wallet does not read, does not
    /// read back.
    fn private_payment_journal(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<PrivatePaymentRecord>, Self::Error>;

    /// Add `records` to `wallet_id`'s journal durably, for payments whose
    /// redrive record was saved before the journal kept them.
    fn journal_private_payments(
        &mut self,
        wallet_id: WalletId,
        records: &[PrivatePaymentRecord],
    ) -> Result<(), Self::Error>;

    /// Remove `wallet_id`'s journal record of the entry `commitment`, once
    /// its payment provably resolved: the entry was read back, or the payment
    /// is proven expired (its anchor pruned and its entry absent from the
    /// store). Returns whether there was one. Only this call and
    /// [`Self::retire_undelivered_spend`] remove a journal record: not a note
    /// release, a redrive's removal or a purge.
    fn discharge_private_payment(
        &mut self,
        wallet_id: WalletId,
        commitment: &[u8; 32],
    ) -> Result<bool, Self::Error>;

    /// Retire the saved record of `id`'s spend `activity_id`, whose
    /// transition never left the wallet, together with the journal records of
    /// `commitments` in `id`'s wallet, the entries it would have appended. One
    /// durable write removes them all (the file store commits it on its
    /// recovery connection, at `synchronous=FULL`), so no restart finds the
    /// journal discharged while the signed transition is still saved, or the
    /// other way round.
    ///
    /// Returns false, changing nothing, when no such record is saved, or when
    /// a re-broadcast of it was claimed ([`PendingRedrive::attempts`] above
    /// 0): the transition may then have left the wallet. The notes the spend
    /// reserved are the caller's to release.
    fn retire_undelivered_spend(
        &mut self,
        id: SubwalletId,
        activity_id: &[u8; 32],
        commitments: &[[u8; 32]],
    ) -> Result<bool, Self::Error>;

    /// Every armed redrive record for `id`.
    fn pending_redrives(&self, id: SubwalletId) -> Result<Vec<PendingRedrive>, Self::Error>;

    /// Every armed redrive record for a wallet, paired with the subwallet
    /// that owns its durable row. Previously bound accounts remain visible
    /// here while they retain a wallet-wide identity-debit guard.
    fn pending_redrives_for_wallet(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<(SubwalletId, PendingRedrive)>, Self::Error>;

    /// Increment the attempt counter on `id`'s redrive keyed by
    /// `activity_id`, returning the new count (`0` when no such record
    /// exists).
    fn bump_redrive_attempts(
        &mut self,
        id: SubwalletId,
        activity_id: &[u8; 32],
    ) -> Result<u32, Self::Error>;

    /// Drop the redrive record keyed by `activity_id` — the spend
    /// resolved (landed, definitively rejected, or released by the
    /// anchor-prune backstop). Implementations also drop the record
    /// implicitly when [`Self::mark_spent`] or [`Self::clear_pending`]
    /// resolves one of its nullifiers, since a transition lands or dies
    /// atomically for all of its nullifiers.
    fn clear_redrive(&mut self, id: SubwalletId, activity_id: &[u8; 32])
        -> Result<(), Self::Error>;

    // ── Outgoing history (per-subwallet) ───────────────────────────────

    /// Record an outgoing (sent) note recovered via OVK for `id`.
    ///
    /// Idempotent by `note.cmx`: re-recording a note already on file
    /// (a re-scan of the same chunk) is a no-op and returns `false`;
    /// a genuinely new outgoing note is stored and returns `true`.
    /// Outgoing notes are append-only send history — there is no
    /// "mark spent" / mutation path for them.
    fn record_outgoing_note(
        &mut self,
        id: SubwalletId,
        note: &ShieldedOutgoingNote,
    ) -> Result<bool, Self::Error>;

    /// Return every outgoing (sent) note recovered for `id`, in the
    /// order they were recorded.
    fn get_outgoing_notes(&self, id: SubwalletId)
        -> Result<Vec<ShieldedOutgoingNote>, Self::Error>;

    // ── Derived activity log (per-subwallet) ───────────────────────────

    /// Upsert a derived [`ShieldedActivityEntry`] for `id`, keyed by
    /// `entry.id` (the sha256 of the visible output cmxs — see
    /// [`crate::wallet::shielded::activity`]).
    ///
    /// Re-saving an entry with the same `entry.id` overwrites the
    /// existing one in place. This is what lets a coarse scan-derived
    /// `ShieldedSpend` be upgraded to a specific kind when a later live
    /// entry (or correlation pass) re-emits the same id, and what lets a
    /// `Pending` entry flip to `Confirmed`/`Failed`.
    fn save_activity(
        &mut self,
        id: SubwalletId,
        entry: &super::activity::ShieldedActivityEntry,
    ) -> Result<(), Self::Error>;

    /// Return a page of derived activity for `id`, sorted for display
    /// (pendings first, then by `block_height` desc, tiebreak by
    /// `created_at_ms` then `id`), sliced by `[offset, offset+limit)`.
    /// `limit == 0` returns an empty page.
    fn get_activity(
        &self,
        id: SubwalletId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<super::activity::ShieldedActivityEntry>, Self::Error>;

    /// Look up a single activity entry by its `entry.id`. `None` if no
    /// entry with that id exists for `id`.
    fn get_activity_by_entry_id(
        &self,
        id: SubwalletId,
        entry_id: &[u8; 32],
    ) -> Result<Option<super::activity::ShieldedActivityEntry>, Self::Error>;

    /// Return the set of all entry ids already recorded for `id`. Used by
    /// the scan deriver to skip clusters a live entry already owns.
    fn get_activity_ids(&self, id: SubwalletId) -> Result<BTreeSet<[u8; 32]>, Self::Error>;

    // ── Commitment tree (network-shared) ───────────────────────────────

    /// Append a note commitment to the shared tree.
    ///
    /// `marked` controls whether shardtree retains the
    /// authentication path for this position (Marked) or prunes it
    /// (Ephemeral). The sync path passes `true` for **every**
    /// position: the tree is a chain-wide structure shared by all
    /// wallets, and wallets bind at different times, so deciding
    /// retention from "is this position owned right now" loses the
    /// ability to witness a note whose owner binds later
    /// (shardtree can't retroactively mark). Per-wallet ownership
    /// is tracked separately in the per-`SubwalletId` note store.
    /// The `marked` parameter is kept for store-level flexibility
    /// and tests.
    fn append_commitment(&mut self, cmx: &[u8; 32], marked: bool) -> Result<(), Self::Error>;

    /// Create a tree checkpoint at the given identifier.
    fn checkpoint_tree(&mut self, checkpoint_id: u32) -> Result<(), Self::Error>;

    /// Return the current tree root (Sinsemilla anchor, 32 bytes).
    fn tree_anchor(&self) -> Result<[u8; 32], Self::Error>;

    /// Generate a Merkle authentication path for `position` as of the
    /// checkpoint at `depth` (0 = current tree state, 1 = the previous
    /// checkpoint, and so on). Returns `Ok(None)` when no witness is
    /// available at that depth — the position is unmarked/pruned, the
    /// requested checkpoint depth doesn't exist, or the position was
    /// appended after that checkpoint.
    ///
    /// Building a spend against an older-but-recorded checkpoint is how the
    /// wallet keeps its anchor consistent with a root Platform actually
    /// recorded: Platform records one anchor per block while an index-chunk
    /// sync routinely leaves the tree mid-block, so the depth-0 root is often
    /// one Platform never recorded.
    fn witness_at_depth(
        &self,
        position: u64,
        depth: usize,
    ) -> Result<Option<grovedb_commitment_tree::MerklePath>, Self::Error>;

    /// Generate a Merkle authentication path for `position` against the
    /// current tree state. Returns `Ok(None)` if no witness is available
    /// (position not marked, or pruned).
    ///
    /// Delegates to [`Self::witness_at_depth`] at depth 0.
    fn witness(
        &self,
        position: u64,
    ) -> Result<Option<grovedb_commitment_tree::MerklePath>, Self::Error> {
        self.witness_at_depth(position, 0)
    }

    /// Number of leaves currently in the shared commitment tree
    /// (= highest appended position + 1, or 0 when empty).
    ///
    /// This is the append watermark for the tree itself, distinct
    /// from any per-subwallet `last_synced_note_index`. The sync
    /// path gates [`Self::append_commitment`] on this value — never
    /// on a per-subwallet watermark — so a re-fetch from a chunk
    /// boundary (forced when a lagging subwallet rewinds the fetch
    /// start) re-appends nothing already in the tree. Double-append
    /// corrupts the shardtree's internal nodes and makes per-position
    /// witnesses resolve against inconsistent roots ("Anchor not
    /// found in the recorded anchors tree" at spend time).
    fn tree_size(&self) -> Result<u64, Self::Error>;

    // ── Sync state (per-subwallet) ─────────────────────────────────────

    /// Sync watermark for `id`: the count of note positions already
    /// scanned, i.e. the next global commitment-tree index to scan.
    /// `0` means nothing scanned yet; `N` means positions `0..N` are
    /// done. Exclusive upper bound — *not* the last index scanned
    /// (scanning through position `N-1` sets this to `N`).
    fn last_synced_note_index(&self, id: SubwalletId) -> Result<u64, Self::Error>;

    /// Persist the sync watermark (next index to scan) for `id`. Only a
    /// pass that reached the tip calls this: the setter also records the
    /// scan coverage and the `ScannedThisSession` balance source behind
    /// [`local_account_balance`](Self::local_account_balance).
    fn set_last_synced_note_index(
        &mut self,
        id: SubwalletId,
        index: u64,
    ) -> Result<(), Self::Error>;

    // ── Per-subwallet lifecycle ────────────────────────────────────────

    /// Drop ALL in-memory per-subwallet state (decrypted notes,
    /// spent marks, `last_synced_note_index`, nullifier
    /// checkpoints, pending reservations) for every subwallet
    /// belonging to `wallet_id`. The shared commitment tree is
    /// left untouched — it's a chain-wide structure, not
    /// per-wallet. Used when a wallet is removed or its shielded
    /// binding is cleared so a later re-bind resyncs from index 0
    /// rather than resuming behind the stale watermark.
    fn purge_wallet(&mut self, wallet_id: WalletId) -> Result<(), Self::Error>;

    /// Drop the account-scoped state and nullifier-backed redrives for exactly
    /// one subwallet, leaving every other subwallet and the shared commitment
    /// tree intact. Empty-nullifier identity-debit guards are wallet-wide and
    /// survive until authenticated resolution or an explicit wallet purge.
    ///
    /// The account-scoped sibling of [`Self::purge_wallet`]. Used by
    /// the coordinator when a re-bind changes a wallet's account set:
    /// only the accounts that were dropped (or whose viewing key
    /// changed — their stored notes belong to the old key) are
    /// purged, so the accounts that remain bound keep their fresh
    /// in-memory notes and watermarks across the re-bind.
    fn purge_subwallet(&mut self, id: SubwalletId) -> Result<(), Self::Error>;

    /// Drop ALL in-memory per-subwallet state for every subwallet
    /// of every wallet. The shared commitment tree is left
    /// untouched. Used by `NetworkShieldedCoordinator::clear()`.
    fn purge_all_subwallets(&mut self) -> Result<(), Self::Error>;

    // ── Removal tombstones ─────────────────────────────────────────────

    /// Record that `wallet_id` has been removed and its
    /// [`purge_wallet`](Self::purge_wallet) is NOT yet confirmed complete.
    ///
    /// Written by `NetworkShieldedCoordinator::unregister_wallet_with` past
    /// its commit point and BEFORE it attempts the purge, then dropped by
    /// [`clear_wallet_removal`](Self::clear_wallet_removal) once that purge
    /// actually succeeds.
    ///
    /// # Why this has to be durable
    ///
    /// The coordinator remembers an unfinished purge in an in-memory
    /// `deferred_purges` set, and a newly opened coordinator starts with that
    /// set EMPTY. A process death between the removal and a successful purge
    /// therefore lost the deferral entirely, leaving this wallet's rows —
    /// decrypted notes, pending claims, activity, and a `last_synced_note_index`
    /// that restore only ever advances — to be inherited by a later
    /// re-registration of the same id, which would then report caught-up and
    /// skip the historical notes the purge existed to force a rescan of
    /// (#4313 review finding 5ca995a0d2aa).
    ///
    /// Written BEFORE the purge rather than only on its failure, because a
    /// crash *during* the purge orphans exactly the same rows and leaves no
    /// failure to react to. A tombstone that outlives a purge which actually
    /// succeeded costs one idempotent no-op purge on a later pass; a missing
    /// one costs the rows forever. Idempotent.
    fn record_wallet_removal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error>;

    /// Drop `wallet_id`'s removal tombstone. Called only once its purge has
    /// completed, so an outstanding tombstone means "this wallet's state may
    /// still be here" and nothing weaker. Idempotent, and a no-op for a wallet
    /// that has none.
    fn clear_wallet_removal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error>;

    /// Every wallet id whose removal tombstone is still outstanding, ascending.
    ///
    /// The durable half of the coordinator's deferred-purge set: it is what a
    /// freshly opened coordinator (a relaunched process) has instead of that
    /// set, and what lets startup, every sync pass, and a re-registration find
    /// orphaned state they never witnessed being orphaned.
    fn outstanding_wallet_removals(&self) -> Result<Vec<WalletId>, Self::Error>;

    /// Empty the shared commitment tree back to zero leaves.
    ///
    /// After this returns, [`Self::tree_size`] reports `0` and the
    /// next [`Self::append_commitment`] starts at position `0`. The
    /// per-subwallet watermarks ([`Self::last_synced_note_index`])
    /// are *not* touched here — callers that want a cold rebuild
    /// pair this with [`Self::purge_all_subwallets`] so the
    /// re-download watermark and the tree reset together.
    ///
    /// Used by `NetworkShieldedCoordinator::clear()` so the host's
    /// "Clear" action is a true cold reset rather than a watermark
    /// rewind into an already-full tree. Without it, Clear leaves
    /// the tree at its full size while the watermark drops to 0, so
    /// every re-fetched position is gate-skipped (`global_pos <
    /// tree_size`) and the "Checked" progress bar stays pinned at
    /// the stale leaf count while "Downloaded" climbs from 0.
    fn reset_commitment_tree(&mut self) -> Result<(), Self::Error>;

    // ── Lifecycle admission (store-level, cross-instance) ──────────────
    //
    // See the [`LifecycleAdmission`] module docs for the protocol and its
    // correctness argument. These five methods exist on the STORE, not on the
    // coordinator, because the store is the only object two coordinators —
    // or two processes — sharing the same backing state actually have in
    // common (`dashpay/platform#4313`).

    /// Admit a one-time-key claim for `wallet_id`, or refuse it because a
    /// destructive lifecycle operation holds admission over that scope.
    ///
    /// On `Ok(true)` a claim lease keyed by `token` is durable and live until
    /// `now_ms + lease_ms`; the caller owns it until it calls
    /// [`Self::end_claim_admission`]. On `Ok(false)` nothing was written and
    /// the caller must not touch the claim record.
    ///
    /// Implementations MUST make the barrier check and the lease insert one
    /// atomic step against every other admission operation on the same
    /// underlying state.
    fn begin_claim_admission(
        &mut self,
        wallet_id: WalletId,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error>;

    /// Arm `redrive` **only if** the claim lease `token` is still live,
    /// re-stamping that lease to `now_ms + lease_ms` in the same atomic step.
    ///
    /// Returns `Ok(false)` — with nothing written — when the lease has expired
    /// or was already released. Callers fail the claim closed on `false`: the
    /// record is the only handle that recovers a padded single-note claim, so
    /// broadcasting without it is unrecoverable.
    ///
    /// Arming under the lease rather than next to it is what closes the gap
    /// between "the claim checked that it was admitted" and "the claim wrote
    /// the record": the two are one transaction, so a destructive operation
    /// cannot slot in between them.
    ///
    /// # The claim-key gate
    ///
    /// The same step ALSO refuses — `Ok(false)`, nothing written — when a live
    /// [claim-key reservation](Self::reserve_one_time_claim_key) covers
    /// `(id.wallet_id, redrive.activity_id)` under a *different* token. That is
    /// what makes the `INSERT OR REPLACE` below structurally unable to clobber
    /// another claimant's record rather than merely unlikely to
    /// (#4313 review finding cr-9d0e1a44): a second claimant that somehow
    /// reached this call without owning the key is stopped at the storage
    /// layer, in the same transaction that would have overwritten the row.
    ///
    /// Ordinary spend redrives are unaffected — they never take a claim-key
    /// reservation, so no live row covers their activity id and the gate is a
    /// no-op for them.
    fn arm_redrive_under_claim(
        &mut self,
        id: SubwalletId,
        redrive: PendingRedrive,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error>;

    /// Reserve this invitation's claim-record key for the holder of claim lease
    /// `token`, atomically, and report who owns it afterwards.
    ///
    /// # Why this exists
    ///
    /// The claim lease taken by [`Self::begin_claim_admission`] is per-WALLET:
    /// it orders a claim against a destructive purge, and nothing else. Two
    /// claims of the SAME invitation are both admitted by it. In-process they
    /// are serialized by the coordinator's per-FVK single-flight mutex — but
    /// that mutex is per-coordinator, and two coordinators (or two processes)
    /// open independent SQLite connections to one file and share no lock at
    /// all. Both then found no pending record, built transitions with
    /// DIFFERENT padded identity ids, and the second `arm_redrive_under_claim`
    /// — an `INSERT OR REPLACE` — silently overwrote the first's byte-exact
    /// recovery row while its transition was already on the wire, stranding
    /// that identity forever (#4313 review finding cr-9d0e1a44).
    ///
    /// So the key itself is reserved where the contention actually is: one
    /// durable row per `(wallet_id, claim_record_key)`, claimed by an
    /// insert that cannot overwrite an existing row.
    ///
    /// # Contract
    ///
    /// Implementations MUST make the insert-if-absent, the read-back of the
    /// durable reservation row, AND the read of the durable pending-claim
    /// record ONE atomic step against every other admission operation on the
    /// same underlying state, and MUST NOT overwrite a live row. The returned
    /// [`ClaimKeyReservationOutcome::reservation`] is derived from the durable
    /// row as it stands after the attempt, so exactly one concurrent caller
    /// can see [`ClaimKeyReservation::Acquired`]:
    ///
    /// * [`ClaimKeyReservation::Acquired`] — this `token` owns the key. Also
    ///   returned when `token` already owned it (re-entry is idempotent and
    ///   re-stamps the row).
    /// * [`ClaimKeyReservation::Held`] — a different, live claimant owns it.
    ///   The caller MUST NOT build, broadcast or arm anything: it either
    ///   RESUMES the durable pending-claim record that claimant left, or
    ///   refuses with [`PlatformWalletError::ShieldedLifecycleBusy`].
    ///
    /// [`ClaimKeyReservationOutcome::pending`] carries the pending-claim record
    /// armed under `claim_record_key` in `claim_records_id`, read from DURABLE
    /// state in that same step — never from a mirror populated at store open.
    /// A caller that receives `Some` resumes it; arming a fresh record over it
    /// is what strands an already-broadcast identity
    /// (#4313 review finding r3767229122, and see the type's docs).
    ///
    /// `claim_records_id` is the reserved subwallet the claim records live
    /// under, so implementations can address the row exactly; the reservation
    /// itself is keyed by `(claim_records_id.wallet_id, claim_record_key)` —
    /// an invitation is contended per WALLET, not per account.
    ///
    /// The reservation is bound to `token` for its whole life: it is re-stamped
    /// by [`Self::renew_claim_admission`] and by [`Self::arm_redrive_under_claim`]
    /// alongside the lease, released by [`Self::end_claim_admission`], and
    /// otherwise reaped by expiry — so a claimant that died without releasing
    /// cannot hold an invitation hostage beyond [`CLAIM_LEASE_MS`].
    ///
    /// [`PlatformWalletError::ShieldedLifecycleBusy`]: crate::error::PlatformWalletError::ShieldedLifecycleBusy
    fn reserve_one_time_claim_key(
        &mut self,
        claim_records_id: SubwalletId,
        claim_record_key: [u8; 32],
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<ClaimKeyReservationOutcome, Self::Error>;

    /// Re-stamp a live claim lease to `now_ms + lease_ms`, WITHOUT touching the
    /// pending record.
    ///
    /// [`Self::arm_redrive_under_claim`] stamps the lease once, so the window
    /// protecting a claim ran for a fixed [`CLAIM_LEASE_MS`] from the arm — not
    /// for as long as the claim actually took. A broadcast plus confirmation
    /// that outran it (a slow or retrying DAPI node is enough) let the row be
    /// reaped, at which point a purge counted zero live claims and destroyed
    /// state the in-flight claim still needed (#4313 review finding 161a517fce36).
    ///
    /// Returns `false` when the lease is gone — already lapsed and reaped, or
    /// displaced by a destructive barrier. The caller cannot un-send a
    /// transition, so a `false` is a loud diagnostic, not an abort.
    ///
    /// Any [claim-key reservation](Self::reserve_one_time_claim_key) held under
    /// the same `token` is re-stamped with it, so the key stays reserved for
    /// exactly as long as the lease that took it.
    fn renew_claim_admission(
        &mut self,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error>;

    /// Release the claim lease `token`, together with any
    /// [claim-key reservation](Self::reserve_one_time_claim_key) it holds.
    /// Idempotent; unknown tokens are a no-op (a lease that already expired was
    /// reaped).
    fn end_claim_admission(&mut self, token: AdmissionToken) -> Result<(), Self::Error>;

    /// Take (or refresh) destructive admission over `scope` and report how
    /// many claim leases are still live inside it.
    ///
    /// `scope` is `None` for a whole-store operation (`purge_all_subwallets`)
    /// and `Some(wallet_id)` for a single wallet (`purge_wallet`). The barrier
    /// is installed **and** the live-lease count taken in one atomic step, so
    /// a claim is either counted here or refused by
    /// [`Self::begin_claim_admission`] — never both, never neither.
    ///
    /// A non-zero count means the caller must wait and call again; it must not
    /// purge. The barrier carries its own expiry so a crashed holder cannot
    /// block claims forever, and refreshing it is exactly re-calling this.
    fn begin_destructive_admission(
        &mut self,
        scope: Option<WalletId>,
        token: AdmissionToken,
        now_ms: u64,
        barrier_ms: u64,
    ) -> Result<usize, Self::Error>;

    /// Drop the destructive barrier `token`, whether the operation went ahead
    /// or gave up. Idempotent.
    fn end_destructive_admission(&mut self, token: AdmissionToken) -> Result<(), Self::Error>;
}

/// How long a one-time-claim lease stays live without being re-stamped.
///
/// It has to comfortably exceed the longest single phase a claim spends between
/// two store touches — the transient full-history scan of a cold wallet, or a
/// Halo 2 proof on a slow phone — because a lease that lapses mid-claim makes
/// the record arming fail closed (safe, but a wasted attempt). It also bounds
/// how long a claim that was CANCELLED without releasing (a dropped JNI call)
/// can block wallet removal, so it must not be unbounded either. Five minutes
/// sits well above both phases and well below a user's patience for "try
/// removing the wallet again".
///
/// The lease is re-stamped when the record is armed
/// ([`ShieldedStore::arm_redrive_under_claim`]) and, independently of that, on
/// a [`CLAIM_LEASE_RENEW_INTERVAL`] heartbeat that the wallet layer runs around
/// the COMPLETE admitted claim — fresh build and pending-record resume alike.
/// So this constant bounds the gap between two renewals, not the length of a
/// claim: the protected window follows the work.
pub(crate) const CLAIM_LEASE_MS: u64 = 5 * 60 * 1_000;

/// How often an in-flight claim re-stamps its lease
/// ([`ShieldedStore::renew_claim_admission`]).
///
/// A third of [`CLAIM_LEASE_MS`]: two consecutive renewals may be missed — a
/// stalled executor, a long blocking store write — before the lease can lapse,
/// while the tick stays far cheaper than the network phases it runs alongside.
/// Renewing is a single indexed UPDATE, so the cost is noise next to a
/// broadcast.
pub(crate) const CLAIM_LEASE_RENEW_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(CLAIM_LEASE_MS / 3);

/// How long a destructive barrier survives without being refreshed.
///
/// Only has to outlive one drain wait (it is refreshed on every poll), plus
/// margin. Kept short so a purge whose process died cannot keep refusing
/// claims for long.
pub(crate) const DESTRUCTIVE_BARRIER_MS: u64 = 60 * 1_000;

/// How long a destructive lifecycle operation waits for in-flight claims to
/// drain before giving up.
///
/// Giving up means REFUSING to purge, not purging anyway: deleting a record
/// under a live claim is the unrecoverable outcome this whole mechanism
/// exists to prevent, while a refused purge is a retry.
pub(crate) const DESTRUCTIVE_DRAIN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Poll interval while waiting for claim leases to drain. Each poll also
/// refreshes the barrier, so no new claim slips in during the wait.
pub(crate) const DESTRUCTIVE_DRAIN_POLL: std::time::Duration =
    std::time::Duration::from_millis(250);

/// Opaque owner token for one lifecycle admission — a claim lease or a
/// destructive barrier.
///
/// 16 random bytes from the OS CSPRNG rather than a counter: admissions are
/// compared across independent store instances and, for the file-backed store,
/// across PROCESSES sharing one SQLite file, so a per-process counter could
/// collide and let one holder release another's admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdmissionToken(pub [u8; 16]);

impl AdmissionToken {
    /// A fresh token from the OS CSPRNG.
    ///
    /// Uses [`RngCore::try_fill_bytes`](rand::RngCore::try_fill_bytes) rather
    /// than `fill_bytes`: the latter *panics* when the OS entropy source
    /// fails. Both production callers are reached from `#[no_mangle] extern
    /// "C"` exports (the claim path through
    /// `platform_wallet_manager_shielded_identity_create_from_one_time_key`,
    /// the barrier path through the destructive lifecycle exports), and a
    /// panic inside those futures is re-raised by `block_on_worker`'s
    /// `expect("tokio worker panicked")` where it cannot unwind across the C
    /// ABI — aborting the host process before the JNI panic guard can run.
    /// Same reasoning, and same remedy, as
    /// [`generate_one_time_orchard_key`](super::keys::generate_one_time_orchard_key).
    ///
    /// Reported as [`PlatformWalletError::Persistence`] because an admission
    /// token is only ever minted to enter the store's admission protocol: the
    /// caller is already mapping that step's failures to `Persistence`, so the
    /// host sees one error class for "the admission could not be taken"
    /// regardless of which half failed. There is deliberately no infallible
    /// `Default`/`new` on this type outside tests — an infallible path would
    /// reintroduce exactly the panic this returns instead.
    pub fn generate() -> Result<Self, crate::error::PlatformWalletError> {
        use rand::{rngs::OsRng, RngCore};

        let mut bytes = [0u8; 16];
        OsRng.try_fill_bytes(&mut bytes).map_err(|e| {
            crate::error::PlatformWalletError::Persistence(format!(
                "OS RNG entropy source failed while minting a lifecycle admission token: {e}"
            ))
        })?;
        Ok(Self(bytes))
    }

    /// Infallible token for tests only — panics on entropy failure, which is
    /// why it is not available to production code (see [`generate`](Self::generate)).
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::generate().expect("OS RNG entropy source failed in a test")
    }
}

/// Wall-clock milliseconds since the Unix epoch — the one clock every
/// admission lease and barrier is stamped and judged against.
///
/// Wall clock rather than a monotonic instant because the leases are compared
/// across processes, which share no monotonic origin. Both holders read the
/// same system clock on the same machine, which is what the comparison needs.
pub fn admission_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One row of the lifecycle-admission table.
///
/// # The protocol, and why it is at the store
///
/// A one-time-key claim and a destructive lifecycle operation (`clear`,
/// `unregister_wallet`, and `remove_wallet` through it) both act on the same
/// durable pending-claim record. `clear` and `unregister_wallet` serialize
/// against each other on the coordinator's `lifecycle` mutex, but a claim never
/// takes it, and the per-FVK single-flight guards are owned by ONE
/// `NetworkShieldedCoordinator` — so a purge could delete an armed record while
/// its transition was still broadcasting. A coordinator-local `tokio` mutex
/// cannot fix that: `FileBackedShieldedStore::open_path` opens independent
/// SQLite connections to the same file, so two coordinators (or two processes)
/// share the state but not the mutex (`dashpay/platform#4313`).
///
/// Admission therefore lives at the only thing they do share — the store:
///
/// 1. A claim takes a **lease** ([`ShieldedStore::begin_claim_admission`]),
///    refused if a barrier already covers its wallet.
/// 2. A destructive operation installs a **barrier**
///    ([`ShieldedStore::begin_destructive_admission`]), which blocks new leases
///    and reports the leases already live in scope. It waits for that count to
///    reach zero and refuses to purge if it does not.
/// 3. The claim arms its record *under* its lease
///    ([`ShieldedStore::arm_redrive_under_claim`]), which re-checks and
///    re-stamps the lease in the same atomic step.
///
/// # Why there is no residual race
///
/// Step 1 and step 2 are each ONE atomic step against the shared state. They
/// therefore have a total order, and both orders are safe:
///
/// * lease commits first → the barrier's count sees it → the purge waits.
/// * barrier commits first → the lease's check sees it → the claim is refused.
///
/// For [`FileBackedShieldedStore`](super::file_store::FileBackedShieldedStore)
/// that atomicity is a `BEGIN IMMEDIATE` SQLite transaction: SQLite admits one
/// writer at a time across every connection **and every process** on the file,
/// so the total order holds exactly where a process-local mutex does not. For
/// [`InMemoryShieldedStore`] the shared object *is* the store, reached through
/// the same `RwLock`, so the write guard supplies the same total order.
///
/// No admission call holds a write transaction across scanning, proof
/// construction, broadcast, or a confirmation wait: each is a handful of
/// statements, and the long phases run between them holding only the lease row.
///
/// # Expiry
///
/// Both kinds carry `expires_at`, because a holder can die (process kill,
/// cancelled coroutine) with no chance to release. Expiry is a liveness
/// backstop only — it never lets a purge delete a record under a *live* claim,
/// it only bounds how long a dead one can block wallet removal, and how long a
/// dead purge can block claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleAdmission {
    /// Owner token.
    pub token: AdmissionToken,
    /// `true` for a destructive barrier, `false` for a claim lease.
    pub destructive: bool,
    /// Scope: `None` is store-wide, `Some(id)` is one wallet.
    pub wallet_id: Option<WalletId>,
    /// Unix millis after which this admission is dead and reapable.
    pub expires_at: u64,
}

impl LifecycleAdmission {
    /// Whether this admission's scope covers `wallet_id`. A store-wide entry
    /// covers every wallet; a wallet-scoped one covers only its own.
    pub fn covers(&self, wallet_id: WalletId) -> bool {
        self.wallet_id.is_none_or(|scoped| scoped == wallet_id)
    }

    /// Whether this admission's scope overlaps `scope` (the same containment
    /// relation as [`Self::covers`], in whichever direction applies).
    pub fn overlaps(&self, scope: Option<WalletId>) -> bool {
        match (self.wallet_id, scope) {
            (None, _) | (_, None) => true,
            (Some(mine), Some(theirs)) => mine == theirs,
        }
    }
}

/// One durable claim-key reservation: the row
/// [`ShieldedStore::reserve_one_time_claim_key`] competes for.
///
/// Separate from [`LifecycleAdmission`] because it answers a different
/// question. A lifecycle admission asks "may a claim run against this wallet at
/// all?" and is contended between claims and purges; this asks "which claimant
/// owns THIS invitation?" and is contended between claims of the same
/// invitation. A claim needs both, and they expire together — the reservation
/// carries its owner's `token` precisely so the lease's renew/release path can
/// keep it in lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimKeyLease {
    /// The wallet the claim runs under.
    pub wallet_id: WalletId,
    /// The invitation's deterministic claim-record key (derived from its
    /// one-time full viewing key).
    pub claim_record_key: [u8; 32],
    /// The claim lease that owns this reservation.
    pub token: AdmissionToken,
    /// Unix millis after which this reservation is dead and reapable.
    pub expires_at: u64,
}

/// The outcome of [`ShieldedStore::reserve_one_time_claim_key`], read back from
/// the durable row after the insert-if-absent attempt.
///
/// Exactly one of any set of concurrent callers can observe
/// [`Self::Acquired`]; every other one observes [`Self::Held`] naming the
/// winner. That asymmetry is the whole point — a losing claimant must RESUME
/// the winner's durable pending-claim record or refuse, never replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKeyReservation {
    /// The requesting token owns the key. Also returned on idempotent
    /// re-entry by the token that already owned it.
    Acquired,
    /// A different, still-live claimant owns the key.
    Held {
        /// The claim lease named on the durable row.
        holder: AdmissionToken,
        /// When that claimant's hold lapses, if it never releases.
        expires_at: u64,
    },
}

impl ClaimKeyReservation {
    /// Whether the requesting token came away owning the key.
    pub fn is_acquired(&self) -> bool {
        matches!(self, Self::Acquired)
    }
}

/// Everything [`ShieldedStore::reserve_one_time_claim_key`] settles in its one
/// atomic step: who owns the invitation's record key, AND the durable
/// pending-claim row that already exists under it (if any).
///
/// # Why the row rides along
///
/// The claim path used to read the pending record separately, through
/// [`ShieldedStore::pending_redrives`]. In the file-backed store that reads an
/// in-memory mirror hydrated once at store OPEN, so a second store instance —
/// a second coordinator, or a second process — could not see a row a peer had
/// armed after that open. The consequence was not a stale read but a lost
/// identity: store A arms a claim, returns `ShieldedBroadcastUnconfirmed` and
/// releases its reservation; store B then acquires the freed reservation,
/// sees an empty mirror, builds a DIFFERENT padded transition and REPLACES
/// A's row via [`ShieldedStore::arm_redrive_under_claim`]. If A's transition
/// executes, its randomized identity id is gone for good
/// (#4313 review finding r3767229122).
///
/// So the row is read from the durable state in the SAME transaction that
/// settles the reservation, and handed back here. A caller that receives
/// `Some` RESUMES that record; it must never arm a fresh one over it.
/// Implementations MUST source `pending` from durable state, never from a
/// startup-hydrated mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimKeyReservationOutcome {
    /// Who owns the invitation's claim-record key after this attempt.
    pub reservation: ClaimKeyReservation,
    /// The durable pending-claim record already armed under
    /// `claim_record_key`, read in the same atomic step. `None` when no
    /// earlier attempt left one.
    pub pending: Option<PendingRedrive>,
}

impl ClaimKeyReservationOutcome {
    /// Whether the requesting token came away owning the key.
    pub fn is_acquired(&self) -> bool {
        self.reservation.is_acquired()
    }
}

// ── Per-subwallet bookkeeping ──────────────────────────────────────────

/// In-flight-spend bookkeeping for a single reserved nullifier.
///
/// A reservation starts life bare (both fields `None`) the moment
/// [`SubwalletState::mark_pending`] excludes the note from selection.
/// Once the spend is actually built, [`SubwalletState::set_pending_spend`]
/// records the Platform-**recorded** anchor it was built against and the
/// linked activity-log entry, so the sync reconcile can later detect a
/// stranded (broadcast-accepted-but-never-landed) spend: once that anchor
/// is pruned from Platform's recorded set the spend can never execute, and
/// with the nullifier still unspent the reservation is provably dead and
/// its note can be freed. Held in memory: a restart keeps the reservations
/// a saved [`PendingRedrive`] restores, and drops the others.
#[derive(Debug, Clone, Default)]
pub(super) struct PendingSpend {
    /// The recorded anchor the spend was built against, once known.
    /// `None` while the note is reserved but the spend isn't built yet.
    pub anchor: Option<[u8; 32]>,
    /// The linked activity-log entry id, so a released reservation can
    /// flip its "Pending" row to "Failed" instead of stranding it.
    pub activity_id: Option<[u8; 32]>,
    /// The spend attempt that reserved the note
    /// ([`ShieldedStore::mark_pending_by`]), whose release clears it only
    /// while it still holds it. `None` for a reservation restored from a
    /// redrive record, which only a resolution of its spend clears.
    pub reservation: Option<u64>,
    /// A proved height at which `anchor` was in Platform's recorded set
    /// ([`PendingRedrive::anchor_height`]), when known.
    pub anchor_height: Option<u64>,
}

/// Per-subwallet note + sync state used by both the in-memory and
/// file-backed stores. Kept in this module so both share the
/// exact same shape and the persister callback can serialize it
/// without re-defining the structure on the host side.
#[derive(Debug, Default, Clone)]
pub(super) struct SubwalletState {
    /// All known notes (spent + unspent), in insertion order.
    pub notes: Vec<ShieldedNote>,
    /// Nullifier → index into `notes`, for O(1) `mark_spent`.
    pub nullifier_index: BTreeMap<[u8; 32], usize>,
    /// Sync watermark: count of note positions scanned = the next
    /// global index to scan (exclusive). `0` = nothing scanned yet.
    pub last_synced_index: u64,
    /// Session-only provenance; durable watermarks are still ordinary u64 rows.
    pub last_scanned_index: Option<u64>,
    pub balance_source: ShieldedBalanceSource,
    /// Nullifiers of notes currently being spent in an in-flight
    /// transition, mapped to the [`PendingSpend`] bookkeeping the
    /// sync reconcile needs. Excluded from `unspent_notes()` so
    /// concurrent callers can't double-select. Held in memory; a
    /// reservation whose spend has saved no record yet dies with the
    /// process, but a reservation belonging to an armed
    /// [`PendingRedrive`], which a spend saves before its first
    /// dispatch, is rehydrated on file-store open (the redrive row
    /// carries its nullifiers), so a spend that may have been broadcast
    /// keeps its notes reserved across restarts.
    pub pending_nullifiers: BTreeMap<[u8; 32], PendingSpend>,
    /// Armed re-drivable unconfirmed spends, keyed by activity id.
    /// See [`PendingRedrive`]. Kept consistent with
    /// `pending_nullifiers`: resolving any of a redrive's nullifiers
    /// (mark_spent / clear_pending) drops the whole record — a
    /// transition lands or dies atomically for all its nullifiers.
    pub redrives: BTreeMap<[u8; 32], PendingRedrive>,
    /// Notes this subwallet SENT, recovered via OVK during the scan.
    /// Append-only send history in recording order.
    pub outgoing_notes: Vec<ShieldedOutgoingNote>,
    /// `cmx` set of recorded outgoing notes, for O(log n) idempotency
    /// on `record_outgoing_note` (Orchard cmx is globally unique).
    pub outgoing_cmx_index: BTreeSet<[u8; 32]>,
    /// Derived activity entries keyed by `entry.id` (sha256 of the
    /// visible output cmxs). Upsert-by-id: a later live entry or a
    /// correlation pass that re-emits the same id overwrites the row.
    pub activity: BTreeMap<[u8; 32], super::activity::ShieldedActivityEntry>,
}

impl SubwalletState {
    /// Save (or overwrite-by-nullifier) a note.
    ///
    /// Re-saving a note with a known nullifier overwrites the
    /// existing entry instead of appending a duplicate — Orchard
    /// nullifiers are globally unique, so a re-scan of the same
    /// chunk shouldn't double-count.
    pub(super) fn save_note(&mut self, note: &ShieldedNote) {
        if let Some(&existing_idx) = self.nullifier_index.get(&note.nullifier) {
            self.notes[existing_idx] = note.clone();
            return;
        }
        let idx = self.notes.len();
        self.nullifier_index.insert(note.nullifier, idx);
        self.notes.push(note.clone());
    }

    /// Borrow the selectable notes so balance reads do not clone note payloads.
    fn unspent_notes_iter(&self) -> impl Iterator<Item = &ShieldedNote> {
        self.notes
            .iter()
            .filter(|note| !note.is_spent && !self.pending_nullifiers.contains_key(&note.nullifier))
    }

    pub(super) fn unspent_notes(&self) -> Vec<ShieldedNote> {
        self.unspent_notes_iter().cloned().collect()
    }

    /// Both stores use the same borrowed, reservation-aware checked sum.
    /// Each caller maps overflow into its own storage error.
    pub(super) fn spendable_balance(&self) -> Option<u64> {
        self.unspent_notes_iter()
            .try_fold(0u64, |total, note| total.checked_add(note.value))
    }

    pub(super) fn all_notes(&self) -> Vec<ShieldedNote> {
        self.notes.clone()
    }

    pub(super) fn mark_spent(&mut self, nullifier: &[u8; 32]) -> MarkSpentOutcome {
        let Some(&idx) = self.nullifier_index.get(nullifier) else {
            return MarkSpentOutcome::default();
        };
        let newly_spent = !self.notes[idx].is_spent;
        self.notes[idx].is_spent = true;
        // Resolve the reservation + redrive record whenever the
        // nullifier is KNOWN, not only on the first unspent→spent
        // transition. A note restored from disk already `is_spent`
        // (its owning transition landed in a prior session) paired with
        // a rehydrated redrive row would otherwise keep a ghost
        // reservation alive and re-broadcast a transition that already
        // executed. Removing a pending reservation on a spent note is
        // always safe — a spent note can't be re-spent. `dropped` is
        // returned so the durable store can mirror the deletion even
        // when `newly_spent` is false (the in-memory drop still
        // happened here).
        self.pending_nullifiers.remove(nullifier);
        let dropped = self.drop_redrives_containing(nullifier);
        MarkSpentOutcome {
            newly_spent,
            dropped_redrives: dropped,
        }
    }

    /// Drop every redrive record that carries `nullifier`, returning
    /// the dropped activity ids (the file store mirrors the deletions
    /// to SQLite). A transition lands or dies atomically for all of
    /// its nullifiers, so resolving one resolves the record.
    pub(super) fn drop_redrives_containing(&mut self, nullifier: &[u8; 32]) -> Vec<[u8; 32]> {
        let dropped: Vec<[u8; 32]> = self
            .redrives
            .iter()
            .filter(|(_, r)| r.nullifiers.contains(nullifier))
            .map(|(id, _)| *id)
            .collect();
        for id in &dropped {
            self.redrives.remove(id);
        }
        dropped
    }

    /// Reserve `nullifier` against an in-flight spend. Returns
    /// `true` if newly added, `false` if it was already reserved.
    /// Re-reserving is a true no-op: an already-armed entry keeps its
    /// anchor/activity link (selection excludes pending nullifiers, so
    /// this shouldn't happen — but a plain `insert` would silently
    /// wipe the release pass's only handle on a stranded spend if it
    /// ever did).
    pub(super) fn mark_pending(&mut self, nullifier: &[u8; 32]) -> bool {
        match self.pending_nullifiers.entry(*nullifier) {
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(PendingSpend::default());
                true
            }
            std::collections::btree_map::Entry::Occupied(_) => false,
        }
    }

    /// [`Self::mark_pending`] for the spend attempt `reservation`, which owns
    /// the reservation from now on.
    pub(super) fn mark_pending_by(&mut self, nullifier: &[u8; 32], reservation: u64) -> bool {
        match self.pending_nullifiers.entry(*nullifier) {
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(PendingSpend {
                    reservation: Some(reservation),
                    ..PendingSpend::default()
                });
                true
            }
            std::collections::btree_map::Entry::Occupied(_) => false,
        }
    }

    /// Release the reservation of `nullifier` only while the spend attempt
    /// `reservation` holds it ([`Self::mark_pending_by`]), with the redrive
    /// records it carries, as [`Self::clear_pending`] does. Returns whether
    /// it was released: another attempt's reservation, or one restored from
    /// a redrive record, stays.
    pub(super) fn clear_pending_reserved_by(
        &mut self,
        nullifier: &[u8; 32],
        reservation: u64,
    ) -> bool {
        if self
            .pending_nullifiers
            .get(nullifier)
            .is_some_and(|pending| pending.reservation == Some(reservation))
        {
            self.clear_pending(nullifier)
        } else {
            false
        }
    }

    /// Attach the built spend's recorded `anchor` and linked
    /// `activity_id` to an existing reservation for `nullifier`.
    /// No-op if the nullifier is no longer pending (e.g. the
    /// reservation was already cleared by a concurrent finalize).
    pub(super) fn set_pending_spend(
        &mut self,
        nullifier: &[u8; 32],
        anchor: [u8; 32],
        activity_id: [u8; 32],
        anchor_height: Option<u64>,
    ) {
        if let Some(pending) = self.pending_nullifiers.get_mut(nullifier) {
            pending.anchor = Some(anchor);
            pending.activity_id = Some(activity_id);
            pending.anchor_height = anchor_height;
        }
    }

    /// Give `height` to each reservation built on `anchor`, and each record
    /// on it, that carries no anchor height
    /// ([`ShieldedStore::note_anchor_recorded`]). Returns the activity ids of
    /// the records given it.
    pub(super) fn note_anchor_recorded(&mut self, anchor: &[u8; 32], height: u64) -> Vec<[u8; 32]> {
        for pending in self.pending_nullifiers.values_mut() {
            if pending.anchor == Some(*anchor) && pending.anchor_height.is_none() {
                pending.anchor_height = Some(height);
            }
        }
        let mut noted = Vec::new();
        for redrive in self.redrives.values_mut() {
            if redrive.anchor == *anchor && redrive.anchor_height.is_none() {
                redrive.anchor_height = Some(height);
                noted.push(redrive.activity_id);
            }
        }
        noted
    }

    /// The activity ids of the records built on `anchor` that carry no anchor
    /// height.
    pub(super) fn redrives_without_anchor_height(&self, anchor: &[u8; 32]) -> Vec<[u8; 32]> {
        self.redrives
            .values()
            .filter(|redrive| redrive.anchor == *anchor && redrive.anchor_height.is_none())
            .map(|redrive| redrive.activity_id)
            .collect()
    }

    /// Reservations that carry a recorded anchor — i.e. built,
    /// broadcast-but-unconfirmed spends. Returns `(nullifier,
    /// anchor, activity_id)` per such entry; reservations still
    /// awaiting a built spend (`anchor: None`) are skipped. The
    /// sync reconcile checks each anchor against Platform's recorded
    /// set and releases the reservation when the anchor is pruned.
    pub(super) fn stale_pending_spends(&self) -> Vec<StalePendingSpend> {
        self.pending_nullifiers
            .iter()
            .filter_map(|(nullifier, spend)| {
                spend
                    .anchor
                    .map(|anchor| (*nullifier, anchor, spend.activity_id, spend.anchor_height))
            })
            .collect()
    }

    /// Release a reservation previously taken via `mark_pending`.
    /// Returns `true` if a matching reservation was actually
    /// removed.
    pub(super) fn clear_pending(&mut self, nullifier: &[u8; 32]) -> bool {
        let removed = self.pending_nullifiers.remove(nullifier).is_some();
        if removed {
            // Releasing a reservation resolves its spend for good
            // (definitive rejection or prune backstop) — the redrive
            // record goes with it.
            self.drop_redrives_containing(nullifier);
        }
        removed
    }

    /// Arm (or overwrite by activity id) a re-drivable record.
    pub(super) fn arm_redrive(&mut self, redrive: PendingRedrive) {
        self.redrives.insert(redrive.activity_id, redrive);
    }

    pub(super) fn pending_redrives(&self) -> Vec<PendingRedrive> {
        self.redrives.values().cloned().collect()
    }

    pub(super) fn identity_redrives(&self) -> Vec<PendingRedrive> {
        self.redrives
            .values()
            .filter(|redrive| redrive.nullifiers.is_empty())
            .cloned()
            .collect()
    }

    /// Current attempt count for `activity_id`'s redrive, if armed.
    pub(super) fn redrive_attempts(&self, activity_id: &[u8; 32]) -> Option<u32> {
        self.redrives.get(activity_id).map(|r| r.attempts)
    }

    /// Bump the attempt counter; `0` when no such record exists.
    pub(super) fn bump_redrive_attempts(&mut self, activity_id: &[u8; 32]) -> u32 {
        self.redrives
            .get_mut(activity_id)
            .map(|r| {
                r.attempts = r.attempts.saturating_add(1);
                r.attempts
            })
            .unwrap_or(0)
    }

    pub(super) fn clear_redrive(&mut self, activity_id: &[u8; 32]) {
        self.redrives.remove(activity_id);
    }

    /// Record an outgoing (sent) note. Idempotent by `cmx`: returns
    /// `true` if newly recorded, `false` if a note with that `cmx`
    /// was already present.
    pub(super) fn record_outgoing_note(&mut self, note: &ShieldedOutgoingNote) -> bool {
        if !self.outgoing_cmx_index.insert(note.cmx) {
            return false;
        }
        self.outgoing_notes.push(note.clone());
        true
    }

    pub(super) fn outgoing_notes(&self) -> Vec<ShieldedOutgoingNote> {
        self.outgoing_notes.clone()
    }

    /// Upsert a derived activity entry by `entry.id`.
    pub(super) fn save_activity(&mut self, entry: &super::activity::ShieldedActivityEntry) {
        self.activity.insert(entry.id, entry.clone());
    }

    /// Display-sorted page of activity entries.
    pub(super) fn activity_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Vec<super::activity::ShieldedActivityEntry> {
        if limit == 0 {
            return Vec::new();
        }
        let mut all: Vec<super::activity::ShieldedActivityEntry> =
            self.activity.values().cloned().collect();
        super::activity::sort_activity_for_display(&mut all);
        all.into_iter().skip(offset).take(limit).collect()
    }

    pub(super) fn activity_by_id(
        &self,
        entry_id: &[u8; 32],
    ) -> Option<super::activity::ShieldedActivityEntry> {
        self.activity.get(entry_id).cloned()
    }

    pub(super) fn activity_ids(&self) -> BTreeSet<[u8; 32]> {
        self.activity.keys().copied().collect()
    }
}

// ── InMemoryShieldedStore ──────────────────────────────────────────────

/// Trivial error type for the in-memory store (infallible in practice).
#[derive(Debug, Clone)]
pub struct InMemoryStoreError(String);

impl fmt::Display for InMemoryStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for InMemoryStoreError {}

/// In-memory implementation of [`ShieldedStore`] for tests and
/// short-lived wallets. Notes are kept per [`SubwalletId`]; the
/// commitment tree is a flat list (anchor is a placeholder, so
/// real witness generation is **not** supported — use a real
/// store for spends).
#[derive(Debug, Default)]
pub struct InMemoryShieldedStore {
    /// Per-subwallet notes + sync state.
    subwallets: BTreeMap<SubwalletId, SubwalletState>,
    /// Flat list of commitments appended to the tree.
    commitments: Vec<[u8; 32]>,
    /// Mark flag per position.
    marked_positions: Vec<bool>,
    /// Checkpoint ids in order.
    checkpoints: Vec<u32>,
    /// Placeholder anchor; production stores compute the real Sinsemilla root.
    anchor: [u8; 32],
    /// The private contact request each armed record pays for, by subwallet
    /// and activity id, read joined with the live records.
    private_contact_payments: BTreeMap<(SubwalletId, [u8; 32]), PrivateContactPayment>,
    /// The private payment journal's records, by wallet and commitment.
    private_payment_journal: BTreeMap<(WalletId, [u8; 32]), PrivatePaymentRecord>,
    /// The wallets whose private payment journal is open.
    private_payment_journals: BTreeSet<WalletId>,
    /// Live lifecycle admissions — see [`LifecycleAdmission`].
    ///
    /// For this store the shared object two coordinators would contend over is
    /// the store itself, reached through one `RwLock<S>`, so holding the table
    /// here gives the same total order between a claim lease and a destructive
    /// barrier that the file store gets from SQLite's single-writer rule.
    admissions: Vec<LifecycleAdmission>,
    /// Live one-time claim-key reservations — see [`ClaimKeyReservation`].
    /// The same total-order argument as `admissions` applies: the `&mut self`
    /// step under the shared `RwLock` is the atomicity the protocol needs.
    claim_key_reservations: Vec<ClaimKeyLease>,
    /// Outstanding wallet-removal tombstones — see
    /// [`ShieldedStore::record_wallet_removal`].
    ///
    /// This store has no file behind it, so "durable" degenerates to "as long
    /// as the store object lives". That is the right shape for the trait
    /// contract anyway: the tombstone outlives the coordinator that wrote it,
    /// which is exactly what a test that rebuilds a coordinator over the same
    /// store exercises. Only the file-backed store survives a real restart.
    removal_tombstones: BTreeSet<WalletId>,
}

impl InMemoryShieldedStore {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push every claim-key reservation owned by `token` out to `expires_at`.
    /// Free function over the field so it can run while a `&mut` borrow of
    /// `admissions` is live.
    fn restamp_claim_key_reservations(
        reservations: &mut [ClaimKeyLease],
        token: AdmissionToken,
        expires_at: u64,
    ) {
        for reservation in reservations.iter_mut().filter(|r| r.token == token) {
            reservation.expires_at = expires_at;
        }
    }
}

impl ShieldedStore for InMemoryShieldedStore {
    type Error = InMemoryStoreError;

    fn save_note(&mut self, id: SubwalletId, note: &ShieldedNote) -> Result<(), Self::Error> {
        self.subwallets.entry(id).or_default().save_note(note);
        Ok(())
    }

    fn get_unspent_notes(&self, id: SubwalletId) -> Result<Vec<ShieldedNote>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::unspent_notes)
            .unwrap_or_default())
    }

    fn spendable_balance(&self, id: SubwalletId) -> Result<u64, Self::Error> {
        self.subwallets
            .get(&id)
            .map_or(Some(0), SubwalletState::spendable_balance)
            .ok_or_else(|| InMemoryStoreError("spendable shielded balance exceeds u64".to_string()))
    }

    fn get_all_notes(&self, id: SubwalletId) -> Result<Vec<ShieldedNote>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::all_notes)
            .unwrap_or_default())
    }

    fn mark_spent(&mut self, id: SubwalletId, nullifier: &[u8; 32]) -> Result<bool, Self::Error> {
        // In-memory store: the redrive map lives in the same
        // `SubwalletState`, so `mark_spent` already dropped any resolved
        // redrives — nothing durable to mirror. Surface `newly_spent`.
        Ok(self
            .subwallets
            .get_mut(&id)
            .map(|sw| sw.mark_spent(nullifier).newly_spent)
            .unwrap_or(false))
    }

    fn mark_pending(&mut self, id: SubwalletId, nullifier: &[u8; 32]) -> Result<bool, Self::Error> {
        Ok(self
            .subwallets
            .entry(id)
            .or_default()
            .mark_pending(nullifier))
    }

    fn clear_pending(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
    ) -> Result<bool, Self::Error> {
        Ok(self
            .subwallets
            .get_mut(&id)
            .map(|sw| sw.clear_pending(nullifier))
            .unwrap_or(false))
    }

    fn mark_pending_by(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        reservation: u64,
    ) -> Result<bool, Self::Error> {
        Ok(self
            .subwallets
            .entry(id)
            .or_default()
            .mark_pending_by(nullifier, reservation))
    }

    fn clear_pending_reserved_by(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        reservation: u64,
    ) -> Result<bool, Self::Error> {
        let released = self
            .subwallets
            .get_mut(&id)
            .is_some_and(|sw| sw.clear_pending_reserved_by(nullifier, reservation));
        Ok(released)
    }

    fn set_pending_spend(
        &mut self,
        id: SubwalletId,
        nullifier: &[u8; 32],
        anchor: [u8; 32],
        activity_id: [u8; 32],
        anchor_height: Option<u64>,
    ) -> Result<(), Self::Error> {
        if let Some(sw) = self.subwallets.get_mut(&id) {
            sw.set_pending_spend(nullifier, anchor, activity_id, anchor_height);
        }
        Ok(())
    }

    fn note_anchor_recorded(
        &mut self,
        id: SubwalletId,
        anchor: &[u8; 32],
        height: u64,
    ) -> Result<(), Self::Error> {
        if let Some(sw) = self.subwallets.get_mut(&id) {
            sw.note_anchor_recorded(anchor, height);
        }
        Ok(())
    }

    fn stale_pending_spends(&self, id: SubwalletId) -> Result<Vec<StalePendingSpend>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::stale_pending_spends)
            .unwrap_or_default())
    }

    fn arm_redrive(&mut self, id: SubwalletId, redrive: PendingRedrive) -> Result<(), Self::Error> {
        self.private_contact_payments
            .remove(&(id, redrive.activity_id));
        self.subwallets.entry(id).or_default().arm_redrive(redrive);
        Ok(())
    }

    fn arm_private_contact_payment(
        &mut self,
        id: SubwalletId,
        redrive: PendingRedrive,
        payment: PrivateContactPayment,
        entries: &[PrivatePaymentEntry],
        anchor_height: u64,
        store_position: Option<u64>,
    ) -> Result<(), Self::Error> {
        if redrive.nullifiers.is_empty() {
            return Err(InMemoryStoreError(
                "a private payment's record spends no note".to_string(),
            ));
        }
        if !entries.is_empty() {
            self.private_payment_journals.insert(id.wallet_id);
        }
        for entry in entries {
            self.private_payment_journal.insert(
                (id.wallet_id, entry.commitment),
                PrivatePaymentRecord {
                    account_index: id.account_index,
                    activity_id: redrive.activity_id,
                    anchor: redrive.anchor,
                    anchor_height: Some(anchor_height),
                    commitment: entry.commitment,
                    payment,
                    nullifiers: redrive.nullifiers.clone(),
                    entry_hash: Some(entry.entry_hash),
                    store_position,
                },
            );
        }
        self.private_contact_payments
            .insert((id, redrive.activity_id), payment);
        self.subwallets.entry(id).or_default().arm_redrive(redrive);
        Ok(())
    }

    fn private_contact_redrives_for_wallet(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<(PendingRedrive, PrivateContactPayment)>, Self::Error> {
        Ok(self
            .pending_redrives_for_wallet(wallet_id)?
            .into_iter()
            .filter_map(|(id, redrive)| {
                self.private_contact_payments
                    .get(&(id, redrive.activity_id))
                    .map(|payment| (redrive, *payment))
            })
            .collect())
    }

    fn private_payment_journal_open(&self, wallet_id: WalletId) -> Result<bool, Self::Error> {
        Ok(self.private_payment_journals.contains(&wallet_id))
    }

    fn open_private_payment_journal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error> {
        self.private_payment_journals.insert(wallet_id);
        Ok(())
    }

    fn private_payment_journal(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<PrivatePaymentRecord>, Self::Error> {
        Ok(self
            .private_payment_journal
            .range((wallet_id, [0u8; 32])..=(wallet_id, [u8::MAX; 32]))
            .map(|(_, record)| record.clone())
            .collect())
    }

    fn journal_private_payments(
        &mut self,
        wallet_id: WalletId,
        records: &[PrivatePaymentRecord],
    ) -> Result<(), Self::Error> {
        if records.iter().any(|record| record.nullifiers.is_empty()) {
            return Err(InMemoryStoreError(
                "a private payment's record spends no note".to_string(),
            ));
        }
        if !records.is_empty() {
            self.private_payment_journals.insert(wallet_id);
        }
        for record in records {
            self.private_payment_journal
                .insert((wallet_id, record.commitment), record.clone());
        }
        Ok(())
    }

    fn discharge_private_payment(
        &mut self,
        wallet_id: WalletId,
        commitment: &[u8; 32],
    ) -> Result<bool, Self::Error> {
        Ok(self
            .private_payment_journal
            .remove(&(wallet_id, *commitment))
            .is_some())
    }

    fn retire_undelivered_spend(
        &mut self,
        id: SubwalletId,
        activity_id: &[u8; 32],
        commitments: &[[u8; 32]],
    ) -> Result<bool, Self::Error> {
        let Some(subwallet) = self
            .subwallets
            .get_mut(&id)
            .filter(|subwallet| subwallet.redrive_attempts(activity_id) == Some(0))
        else {
            return Ok(false);
        };
        subwallet.clear_redrive(activity_id);
        self.private_contact_payments.remove(&(id, *activity_id));
        for commitment in commitments {
            self.private_payment_journal
                .remove(&(id.wallet_id, *commitment));
        }
        Ok(true)
    }

    fn pending_redrives(&self, id: SubwalletId) -> Result<Vec<PendingRedrive>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::pending_redrives)
            .unwrap_or_default())
    }

    fn pending_redrives_for_wallet(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<(SubwalletId, PendingRedrive)>, Self::Error> {
        Ok(self
            .subwallets
            .iter()
            .filter(|(id, _)| id.wallet_id == wallet_id)
            .flat_map(|(id, subwallet)| {
                subwallet
                    .pending_redrives()
                    .into_iter()
                    .map(|redrive| (*id, redrive))
            })
            .collect())
    }

    fn bump_redrive_attempts(
        &mut self,
        id: SubwalletId,
        activity_id: &[u8; 32],
    ) -> Result<u32, Self::Error> {
        Ok(self
            .subwallets
            .get_mut(&id)
            .map(|sw| sw.bump_redrive_attempts(activity_id))
            .unwrap_or(0))
    }

    fn clear_redrive(
        &mut self,
        id: SubwalletId,
        activity_id: &[u8; 32],
    ) -> Result<(), Self::Error> {
        if let Some(sw) = self.subwallets.get_mut(&id) {
            sw.clear_redrive(activity_id);
        }
        Ok(())
    }

    fn record_outgoing_note(
        &mut self,
        id: SubwalletId,
        note: &ShieldedOutgoingNote,
    ) -> Result<bool, Self::Error> {
        Ok(self
            .subwallets
            .entry(id)
            .or_default()
            .record_outgoing_note(note))
    }

    fn get_outgoing_notes(
        &self,
        id: SubwalletId,
    ) -> Result<Vec<ShieldedOutgoingNote>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::outgoing_notes)
            .unwrap_or_default())
    }

    fn save_activity(
        &mut self,
        id: SubwalletId,
        entry: &super::activity::ShieldedActivityEntry,
    ) -> Result<(), Self::Error> {
        self.subwallets.entry(id).or_default().save_activity(entry);
        Ok(())
    }

    fn get_activity(
        &self,
        id: SubwalletId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<super::activity::ShieldedActivityEntry>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(|sw| sw.activity_page(offset, limit))
            .unwrap_or_default())
    }

    fn get_activity_by_entry_id(
        &self,
        id: SubwalletId,
        entry_id: &[u8; 32],
    ) -> Result<Option<super::activity::ShieldedActivityEntry>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .and_then(|sw| sw.activity_by_id(entry_id)))
    }

    fn get_activity_ids(&self, id: SubwalletId) -> Result<BTreeSet<[u8; 32]>, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(SubwalletState::activity_ids)
            .unwrap_or_default())
    }

    fn append_commitment(&mut self, cmx: &[u8; 32], marked: bool) -> Result<(), Self::Error> {
        self.commitments.push(*cmx);
        self.marked_positions.push(marked);
        Ok(())
    }

    fn checkpoint_tree(&mut self, checkpoint_id: u32) -> Result<(), Self::Error> {
        self.checkpoints.push(checkpoint_id);
        Ok(())
    }

    fn tree_anchor(&self) -> Result<[u8; 32], Self::Error> {
        Ok(self.anchor)
    }

    fn witness_at_depth(
        &self,
        _position: u64,
        _depth: usize,
    ) -> Result<Option<grovedb_commitment_tree::MerklePath>, Self::Error> {
        Err(InMemoryStoreError(
            "Merkle witness not supported in in-memory store".into(),
        ))
    }

    fn tree_size(&self) -> Result<u64, Self::Error> {
        Ok(self.commitments.len() as u64)
    }

    fn last_synced_note_index(&self, id: SubwalletId) -> Result<u64, Self::Error> {
        Ok(self
            .subwallets
            .get(&id)
            .map(|sw| sw.last_synced_index)
            .unwrap_or(0))
    }

    fn set_last_synced_note_index(
        &mut self,
        id: SubwalletId,
        index: u64,
    ) -> Result<(), Self::Error> {
        let state = self.subwallets.entry(id).or_default();
        state.last_synced_index = index;
        state.last_scanned_index = Some(index);
        state.balance_source = ShieldedBalanceSource::ScannedThisSession;
        Ok(())
    }

    fn purge_wallet(&mut self, wallet_id: WalletId) -> Result<(), Self::Error> {
        // The private payment journal outlives the purge: the payments it
        // keeps may still execute.
        self.subwallets.retain(|id, _| id.wallet_id != wallet_id);
        self.private_contact_payments
            .retain(|(id, _), _| id.wallet_id != wallet_id);
        Ok(())
    }

    fn purge_subwallet(&mut self, id: SubwalletId) -> Result<(), Self::Error> {
        let identity_redrives = self
            .subwallets
            .get(&id)
            .map(SubwalletState::identity_redrives)
            .unwrap_or_default();
        self.subwallets.remove(&id);
        if !identity_redrives.is_empty() {
            let retained = self.subwallets.entry(id).or_default();
            for redrive in identity_redrives {
                retained.arm_redrive(redrive);
            }
        }
        Ok(())
    }

    fn purge_all_subwallets(&mut self) -> Result<(), Self::Error> {
        self.subwallets.clear();
        self.private_contact_payments.clear();
        Ok(())
    }

    fn record_wallet_removal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error> {
        self.removal_tombstones.insert(wallet_id);
        Ok(())
    }

    fn clear_wallet_removal(&mut self, wallet_id: WalletId) -> Result<(), Self::Error> {
        self.removal_tombstones.remove(&wallet_id);
        Ok(())
    }

    fn outstanding_wallet_removals(&self) -> Result<Vec<WalletId>, Self::Error> {
        Ok(self.removal_tombstones.iter().copied().collect())
    }

    fn reset_commitment_tree(&mut self) -> Result<(), Self::Error> {
        // Flat-list backing: clearing the commitment / mark /
        // checkpoint vectors drops `tree_size()` to 0 and makes the
        // next append start at position 0, matching the file-backed
        // store's reset contract.
        self.commitments.clear();
        self.marked_positions.clear();
        self.checkpoints.clear();
        self.anchor = [0u8; 32];
        Ok(())
    }

    // ── Lifecycle admission ────────────────────────────────────────────
    //
    // Each of these is one uninterruptible `&mut self` step, and every caller
    // reaches this store through the same `RwLock<S>`, so the write guard
    // supplies exactly the total order the protocol needs — see
    // [`LifecycleAdmission`].

    fn begin_claim_admission(
        &mut self,
        wallet_id: WalletId,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error> {
        self.admissions.retain(|a| a.expires_at > now_ms);
        if self
            .admissions
            .iter()
            .any(|a| a.destructive && a.covers(wallet_id))
        {
            return Ok(false);
        }
        self.admissions.retain(|a| a.token != token);
        self.admissions.push(LifecycleAdmission {
            token,
            destructive: false,
            wallet_id: Some(wallet_id),
            expires_at: now_ms.saturating_add(lease_ms),
        });
        Ok(true)
    }

    fn arm_redrive_under_claim(
        &mut self,
        id: SubwalletId,
        redrive: PendingRedrive,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error> {
        // The claim-key gate, checked BEFORE the lease so a claimant that lost
        // the key can never reach the `arm_redrive` overwrite below — see the
        // trait docs. A live row under a different token means someone else
        // owns this invitation.
        if self.claim_key_reservations.iter().any(|r| {
            r.wallet_id == id.wallet_id
                && r.claim_record_key == redrive.activity_id
                && r.expires_at > now_ms
                && r.token != token
        }) {
            return Ok(false);
        }
        let Some(lease) = self
            .admissions
            .iter_mut()
            .find(|a| a.token == token && !a.destructive && a.expires_at > now_ms)
        else {
            return Ok(false);
        };
        lease.expires_at = now_ms.saturating_add(lease_ms);
        Self::restamp_claim_key_reservations(
            &mut self.claim_key_reservations,
            token,
            now_ms.saturating_add(lease_ms),
        );
        self.subwallets.entry(id).or_default().arm_redrive(redrive);
        Ok(true)
    }

    fn reserve_one_time_claim_key(
        &mut self,
        claim_records_id: SubwalletId,
        claim_record_key: [u8; 32],
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<ClaimKeyReservationOutcome, Self::Error> {
        let wallet_id = claim_records_id.wallet_id;
        // Reap first, so a dead claimant's row cannot hold an invitation
        // hostage past its lease. One `&mut self` step covers reap, insert and
        // read-back, which is the atomicity the trait requires.
        self.claim_key_reservations
            .retain(|r| r.expires_at > now_ms);
        let expires_at = now_ms.saturating_add(lease_ms);
        let reservation = match self
            .claim_key_reservations
            .iter_mut()
            .find(|r| r.wallet_id == wallet_id && r.claim_record_key == claim_record_key)
        {
            // Someone else's live row: read it back and report the holder. The
            // row is NOT touched — that is the whole guarantee.
            Some(existing) if existing.token != token => ClaimKeyReservation::Held {
                holder: existing.token,
                expires_at: existing.expires_at,
            },
            // Our own row: idempotent re-entry, re-stamped.
            Some(existing) => {
                existing.expires_at = expires_at;
                ClaimKeyReservation::Acquired
            }
            None => {
                self.claim_key_reservations.push(ClaimKeyLease {
                    wallet_id,
                    claim_record_key,
                    token,
                    expires_at,
                });
                ClaimKeyReservation::Acquired
            }
        };
        // Read the pending-claim record in the SAME `&mut self` step. This
        // store keeps no separate durable tier — the map IS the durable state —
        // so reading it here is the atomicity the trait asks for, and the
        // file-backed store's SQLite read is the equivalent
        // (#4313 review finding r3767229122).
        let pending = self
            .subwallets
            .get(&claim_records_id)
            .and_then(|sw| sw.redrives.get(&claim_record_key).cloned());
        Ok(ClaimKeyReservationOutcome {
            reservation,
            pending,
        })
    }

    fn renew_claim_admission(
        &mut self,
        token: AdmissionToken,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<bool, Self::Error> {
        // Same predicate arm_redrive_under_claim uses: a live, non-destructive
        // lease under this exact token. Deliberately does NOT resurrect an
        // expired one — a lapsed lease may already have been counted as absent
        // by a purge, and silently reviving it would hide that.
        let Some(lease) = self
            .admissions
            .iter_mut()
            .find(|a| a.token == token && !a.destructive && a.expires_at > now_ms)
        else {
            return Ok(false);
        };
        lease.expires_at = now_ms.saturating_add(lease_ms);
        // Keep the claim-key reservation in lockstep with the lease that owns
        // it, so a long claim does not lose its invitation to expiry while its
        // lease is being kept alive.
        Self::restamp_claim_key_reservations(
            &mut self.claim_key_reservations,
            token,
            now_ms.saturating_add(lease_ms),
        );
        Ok(true)
    }

    fn end_claim_admission(&mut self, token: AdmissionToken) -> Result<(), Self::Error> {
        self.admissions
            .retain(|a| a.destructive || a.token != token);
        self.claim_key_reservations.retain(|r| r.token != token);
        Ok(())
    }

    fn begin_destructive_admission(
        &mut self,
        scope: Option<WalletId>,
        token: AdmissionToken,
        now_ms: u64,
        barrier_ms: u64,
    ) -> Result<usize, Self::Error> {
        self.admissions.retain(|a| a.expires_at > now_ms);
        self.admissions.retain(|a| a.token != token);
        self.admissions.push(LifecycleAdmission {
            token,
            destructive: true,
            wallet_id: scope,
            expires_at: now_ms.saturating_add(barrier_ms),
        });
        Ok(self
            .admissions
            .iter()
            .filter(|a| !a.destructive && a.overlaps(scope))
            .count())
    }

    fn end_destructive_admission(&mut self, token: AdmissionToken) -> Result<(), Self::Error> {
        self.admissions
            .retain(|a| !a.destructive || a.token != token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(account: u32) -> SubwalletId {
        SubwalletId::new([0xAA; 32], account)
    }

    #[test]
    fn should_borrow_spendable_notes_and_preserve_the_selection_filter() {
        let mut state = SubwalletState::default();
        for tag in 1..=4 {
            let mut note = note_with_nullifier([tag; 32]);
            note.value = u64::from(tag) * 100;
            state.save_note(&note);
        }
        state.mark_pending(&[2; 32]);
        state.mark_spent(&[3; 32]);

        let borrowed: Vec<_> = state.unspent_notes_iter().collect();
        assert_eq!(borrowed.len(), 2);
        assert!(std::ptr::eq(borrowed[0], &state.notes[0]));
        assert!(std::ptr::eq(borrowed[1], &state.notes[3]));
        assert_eq!(state.spendable_balance(), Some(500));
        assert_eq!(
            state
                .unspent_notes()
                .iter()
                .map(|note| note.value)
                .collect::<Vec<_>>(),
            vec![100, 400]
        );
        state.clear_pending(&[2; 32]);
        assert_eq!(state.spendable_balance(), Some(700));
    }

    #[test]
    fn should_reject_overflowing_in_memory_spendable_balance() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        for (tag, value) in [(1, u64::MAX), (2, 1)] {
            store
                .save_note(
                    id,
                    &ShieldedNote {
                        position: tag as u64,
                        cmx: [tag; 32],
                        nullifier: [tag; 32],
                        block_height: 100,
                        is_spent: false,
                        value,
                        note_data: vec![],
                    },
                )
                .expect("save test note");
        }
        let error = store
            .spendable_balance(id)
            .expect_err("overflow must not wrap or panic");
        assert!(error.to_string().contains("exceeds u64"));

        // Reservations and spent notes are excluded before checked addition.
        store
            .mark_pending(id, &[2; 32])
            .expect("reserve small note");
        assert_eq!(store.spendable_balance(id).unwrap(), u64::MAX);
        assert_eq!(store.spendable_balance(test_id(1)).unwrap(), 0);
        assert_eq!(
            store
                .spendable_balance(SubwalletId::new([0xBB; 32], 0))
                .unwrap(),
            0
        );
        store
            .clear_pending(id, &[2; 32])
            .expect("release reservation");
        assert!(store.spendable_balance(id).is_err());
        store.mark_spent(id, &[2; 32]).expect("spend small note");
        assert_eq!(store.spendable_balance(id).unwrap(), u64::MAX);
        store
            .mark_spent(id, &[1; 32])
            .expect("spend remaining note");
        assert_eq!(store.spendable_balance(id).unwrap(), 0);
    }

    #[test]
    fn test_save_and_retrieve_notes() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let note = ShieldedNote {
            position: 42,
            cmx: [1u8; 32],
            nullifier: [2u8; 32],
            block_height: 100,
            is_spent: false,
            value: 1000,
            note_data: vec![0u8; 115],
        };
        store.save_note(id, &note).unwrap();

        let unspent = store.get_unspent_notes(id).unwrap();
        assert_eq!(unspent.len(), 1);
        assert_eq!(unspent[0].value, 1000);
        assert_eq!(unspent[0].position, 42);

        // A different subwallet sees no notes.
        let other = test_id(1);
        assert!(store.get_unspent_notes(other).unwrap().is_empty());
    }

    #[test]
    fn test_mark_spent() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [3u8; 32];
        let note = ShieldedNote {
            position: 0,
            cmx: [1u8; 32],
            nullifier,
            block_height: 50,
            is_spent: false,
            value: 500,
            note_data: vec![0u8; 115],
        };
        store.save_note(id, &note).unwrap();

        assert!(store.mark_spent(id, &nullifier).unwrap());
        assert!(store.get_unspent_notes(id).unwrap().is_empty());
        let all = store.get_all_notes(id).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].is_spent);
        // Marking again returns false (already spent).
        assert!(!store.mark_spent(id, &nullifier).unwrap());
    }

    /// Two notes carrying one nullifier must count once.
    ///
    /// A nullifier is what makes a note spendable exactly once, so if two notes in a pool
    /// share one, only one of them can ever be spent and the other's value is unreachable.
    /// A wallet that added both to its balance would show funds it cannot move and would
    /// build transitions that fail against the chain.
    ///
    /// `save_note` keys on the nullifier and overwrites, which gives the right answer. That
    /// behaviour was written to make rescanning idempotent, and its reasoning — that
    /// nullifiers are unique — is exactly what does not hold here. This pins the outcome
    /// rather than the reasoning, so the guarantee survives a future rescan mechanism that
    /// makes the original justification obsolete.
    #[test]
    fn should_count_two_notes_sharing_a_nullifier_once() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [7u8; 32];
        let note = ShieldedNote {
            position: 3,
            cmx: [9u8; 32],
            nullifier,
            block_height: 10,
            is_spent: false,
            value: 1_000,
            note_data: vec![0u8; 115],
        };
        // The same note observed again at a later tree position: same commitment, same
        // nullifier, different provenance.
        let twin = ShieldedNote {
            position: 11,
            block_height: 12,
            ..note.clone()
        };

        store.save_note(id, &note).unwrap();
        store.save_note(id, &twin).unwrap();

        assert_eq!(
            store.get_all_notes(id).unwrap().len(),
            1,
            "a second note sharing a nullifier must replace the first, not join it"
        );
        assert_eq!(
            store.spendable_balance(id).unwrap(),
            1_000,
            "BALANCE INFLATION: the wallet counted both notes, but only one is spendable, \
             so it would offer the holder funds no transition can move"
        );

        // And the single entry really is the one note: spending it leaves nothing behind.
        assert!(store.mark_spent(id, &nullifier).unwrap());
        assert!(
            store.get_unspent_notes(id).unwrap().is_empty(),
            "spending the shared nullifier must leave no second note live"
        );
    }

    /// Resolving any of a redrive's nullifiers — landing (`mark_spent`)
    /// or release (`clear_pending`) — drops the whole record: a
    /// transition lands or dies atomically for all its nullifiers.
    #[test]
    fn resolving_a_nullifier_drops_the_redrive_record() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let n1 = [3u8; 32];
        let n2 = [4u8; 32];
        let note = ShieldedNote {
            position: 0,
            cmx: [1u8; 32],
            nullifier: n1,
            block_height: 50,
            is_spent: false,
            value: 500,
            note_data: vec![0u8; 115],
        };
        store.save_note(id, &note).unwrap();
        let redrive = PendingRedrive {
            activity_id: [9u8; 32],
            anchor: [8u8; 32],
            nullifiers: vec![n1, n2],
            st_bytes: vec![1, 2, 3],
            attempts: 0,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
            identity_index: None,
        };

        // Landing path: mark_spent on one nullifier drops the record.
        store.mark_pending(id, &n1).unwrap();
        store.arm_redrive(id, redrive.clone()).unwrap();
        assert_eq!(store.pending_redrives(id).unwrap().len(), 1);
        assert!(store.mark_spent(id, &n1).unwrap());
        assert!(
            store.pending_redrives(id).unwrap().is_empty(),
            "landed spend drops its redrive record"
        );

        // Release path: clear_pending on a nullifier drops the record.
        store.mark_pending(id, &n2).unwrap();
        store.arm_redrive(id, redrive).unwrap();
        assert!(store.clear_pending(id, &n2).unwrap());
        assert!(
            store.pending_redrives(id).unwrap().is_empty(),
            "released reservation drops its redrive record"
        );
    }

    #[test]
    fn test_sync_state_per_subwallet() {
        let mut store = InMemoryShieldedStore::new();
        let a = test_id(0);
        let b = test_id(1);

        assert_eq!(store.last_synced_note_index(a).unwrap(), 0);
        store.set_last_synced_note_index(a, 100).unwrap();
        assert_eq!(store.last_synced_note_index(a).unwrap(), 100);
        // Different subwallet still at 0.
        assert_eq!(store.last_synced_note_index(b).unwrap(), 0);
    }

    #[test]
    fn test_commitment_tree_operations() {
        let mut store = InMemoryShieldedStore::new();
        store.append_commitment(&[1u8; 32], true).unwrap();
        store.append_commitment(&[2u8; 32], false).unwrap();
        store.checkpoint_tree(1).unwrap();
        assert_eq!(store.tree_anchor().unwrap(), [0u8; 32]);
    }

    #[test]
    fn test_save_activity_upserts_and_paginates() {
        use super::super::activity::{
            ShieldedActivityEntry, ShieldedActivityKind, ShieldedActivityStatus, ShieldedDirection,
        };

        fn entry(
            id: u8,
            height: Option<u64>,
            status: ShieldedActivityStatus,
        ) -> ShieldedActivityEntry {
            ShieldedActivityEntry {
                id: [id; 32],
                kind: ShieldedActivityKind::Sent,
                direction: ShieldedDirection::Out,
                amount: 1,
                fee: None,
                counterparty: None,
                memo: None,
                block_height: height,
                status,
                created_at_ms: 0,
                min_note_position: None,
                note_cmxs: vec![[id; 32]],
                spent_nullifiers: vec![],
            }
        }

        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);

        // Two confirmed at different heights + one pending.
        store
            .save_activity(id, &entry(1, Some(10), ShieldedActivityStatus::Confirmed))
            .unwrap();
        store
            .save_activity(id, &entry(2, Some(20), ShieldedActivityStatus::Confirmed))
            .unwrap();
        store
            .save_activity(id, &entry(3, None, ShieldedActivityStatus::Pending))
            .unwrap();

        // Display order: pending first, then height desc.
        let page = store.get_activity(id, 0, 10).unwrap();
        assert_eq!(page.len(), 3);
        assert_eq!(page[0].id, [3u8; 32], "pending floats to top");
        assert_eq!(page[1].id, [2u8; 32], "height 20 before 10");
        assert_eq!(page[2].id, [1u8; 32]);

        // Upsert by id: re-saving entry 1 as Pending replaces it in place
        // (still 3 entries, but now 1 is pending and floats up).
        store
            .save_activity(id, &entry(1, None, ShieldedActivityStatus::Pending))
            .unwrap();
        let page = store.get_activity(id, 0, 10).unwrap();
        assert_eq!(page.len(), 3, "upsert by id, not append");
        assert_eq!(
            store.get_activity_ids(id).unwrap().len(),
            3,
            "still exactly three distinct ids"
        );

        // Pagination: offset/limit slice the display-sorted list.
        let first_two = store.get_activity(id, 0, 2).unwrap();
        assert_eq!(first_two.len(), 2);
        let last_one = store.get_activity(id, 2, 10).unwrap();
        assert_eq!(last_one.len(), 1);
        // limit 0 => empty page.
        assert!(store.get_activity(id, 0, 0).unwrap().is_empty());

        // Lookup by entry id.
        assert!(store
            .get_activity_by_entry_id(id, &[2u8; 32])
            .unwrap()
            .is_some());
        assert!(store
            .get_activity_by_entry_id(id, &[9u8; 32])
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_reset_commitment_tree_empties_and_reappends_from_zero() {
        let mut store = InMemoryShieldedStore::new();
        store.append_commitment(&[1u8; 32], true).unwrap();
        store.append_commitment(&[2u8; 32], true).unwrap();
        store.checkpoint_tree(2).unwrap();
        assert_eq!(store.tree_size().unwrap(), 2);

        store.reset_commitment_tree().unwrap();
        assert_eq!(
            store.tree_size().unwrap(),
            0,
            "tree_size must be 0 after reset"
        );

        // Re-append starts from position 0 again.
        store.append_commitment(&[3u8; 32], true).unwrap();
        assert_eq!(store.tree_size().unwrap(), 1);
    }

    /// Build a minimal unspent note carrying `nullifier`.
    fn note_with_nullifier(nullifier: [u8; 32]) -> ShieldedNote {
        ShieldedNote {
            position: 0,
            cmx: [1u8; 32],
            nullifier,
            block_height: 1,
            is_spent: false,
            value: 1_000,
            note_data: vec![0u8; 115],
        }
    }

    /// A reservation carrying a recorded anchor surfaces via
    /// `stale_pending_spends`, and the sync reconcile's release decision
    /// (release iff the anchor is absent from the recorded set) must RETAIN
    /// it while the anchor is still recorded — a slow-but-landing spend is
    /// never freed, so its note stays out of the unspent set.
    #[test]
    fn pending_spend_with_recorded_anchor_is_retained() {
        use std::collections::HashSet;

        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [0x11; 32];
        let anchor = [0x22; 32];
        let activity_id = [0x33; 32];

        store
            .save_note(id, &note_with_nullifier(nullifier))
            .unwrap();
        assert!(
            store.mark_pending(id, &nullifier).unwrap(),
            "newly reserved"
        );
        store
            .set_pending_spend(id, &nullifier, anchor, activity_id, Some(10))
            .unwrap();

        assert_eq!(
            store.stale_pending_spends(id).unwrap(),
            vec![(nullifier, anchor, Some(activity_id), Some(10))],
            "an armed reservation surfaces as an anchored (stale-checkable) spend"
        );

        // Anchor IS still recorded → retain.
        let recorded: HashSet<[u8; 32]> = [anchor].into_iter().collect();
        for (n, a, _, _) in store.stale_pending_spends(id).unwrap() {
            if !recorded.contains(&a) {
                store.clear_pending(id, &n).unwrap();
            }
        }
        assert_eq!(
            store.stale_pending_spends(id).unwrap().len(),
            1,
            "a still-recorded anchor must not be released"
        );
        assert!(
            store.get_unspent_notes(id).unwrap().is_empty(),
            "the retained note stays excluded from selection candidates"
        );
    }

    /// A journal record's unknown anchor height is `None` in memory and
    /// `u64::MAX` only in its bytes, as format 3 keeps it; a known one reads
    /// back as it was.
    #[test]
    fn should_keep_an_unknown_journal_anchor_height_as_none_across_its_bytes() {
        let record = |anchor_height| PrivatePaymentRecord {
            account_index: 2,
            activity_id: [0x61; 32],
            anchor: [0x62; 32],
            anchor_height,
            commitment: [0x63; 32],
            payment: PrivateContactPayment {
                sender: [0x64; 32],
                recipient: [0x65; 32],
                account_reference: 3,
            },
            nullifiers: vec![[0x66; 32]],
            entry_hash: None,
            store_position: None,
        };
        for anchor_height in [None, Some(0), Some(41)] {
            let bytes = record(anchor_height).to_bytes().expect("encodes");
            let height_at = 1 + 4 + 32 + 32;
            assert_eq!(
                bytes[height_at..height_at + 8],
                anchor_height.unwrap_or(u64::MAX).to_le_bytes(),
                "{anchor_height:?}"
            );
            assert_eq!(
                PrivatePaymentRecord::from_bytes(&bytes),
                Some(record(anchor_height)),
                "{anchor_height:?}"
            );
        }
    }

    /// A format 4 journal record keeps its entry's hash and its store
    /// position across its bytes, each present or not; a format 3 record,
    /// which kept neither, reads back with neither. A presence byte other than
    /// 0 or 1, or bytes past the last field, do not read.
    #[test]
    fn should_keep_a_journal_record_s_entry_hash_and_position_across_its_bytes() {
        let record = |entry_hash, store_position| PrivatePaymentRecord {
            account_index: 2,
            activity_id: [0x61; 32],
            anchor: [0x62; 32],
            anchor_height: Some(41),
            commitment: [0x63; 32],
            payment: PrivateContactPayment {
                sender: [0x64; 32],
                recipient: [0x65; 32],
                account_reference: 3,
            },
            nullifiers: vec![[0x66; 32], [0x68; 32]],
            entry_hash,
            store_position,
        };
        for (entry_hash, store_position) in [
            (None, None),
            (Some([0x67; 32]), None),
            (None, Some(9)),
            (Some([0x67; 32]), Some(u64::MAX - 1)),
        ] {
            let bytes = record(entry_hash, store_position)
                .to_bytes()
                .expect("encodes");
            assert_eq!(
                PrivatePaymentRecord::from_bytes(&bytes),
                Some(record(entry_hash, store_position)),
                "{entry_hash:?} {store_position:?}"
            );
            let mut longer = bytes;
            longer.push(0);
            assert_eq!(PrivatePaymentRecord::from_bytes(&longer), None);
        }
        // Format 3: the fields up to the nullifiers.
        let mut format_3 = record(None, None).to_bytes().expect("encodes");
        format_3.truncate(format_3.len() - 2);
        format_3[0] = 3;
        assert_eq!(
            PrivatePaymentRecord::from_bytes(&format_3),
            Some(record(None, None))
        );
        format_3.push(0);
        assert_eq!(PrivatePaymentRecord::from_bytes(&format_3), None);
        let mut unknown_presence = record(None, None).to_bytes().expect("encodes");
        let at = unknown_presence.len() - 2;
        unknown_presence[at] = 2;
        assert_eq!(PrivatePaymentRecord::from_bytes(&unknown_presence), None);
    }

    /// The fund-safe release: once the anchor is pruned (absent from the
    /// recorded set) the spend can never execute, so the reservation is
    /// released and the freed note becomes spendable again.
    #[test]
    fn pending_spend_with_pruned_anchor_is_released() {
        use std::collections::HashSet;

        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [0x11; 32];
        let anchor = [0x22; 32];
        let activity_id = [0x33; 32];

        store
            .save_note(id, &note_with_nullifier(nullifier))
            .unwrap();
        store.mark_pending(id, &nullifier).unwrap();
        store
            .set_pending_spend(id, &nullifier, anchor, activity_id, Some(10))
            .unwrap();
        assert!(store.get_unspent_notes(id).unwrap().is_empty());

        // Anchor is ABSENT from the recorded set (pruned) → release.
        let recorded: HashSet<[u8; 32]> = [[0xEE; 32]].into_iter().collect();
        for (n, a, _, _) in store.stale_pending_spends(id).unwrap() {
            if !recorded.contains(&a) {
                assert!(store.clear_pending(id, &n).unwrap());
            }
        }
        assert!(
            store.stale_pending_spends(id).unwrap().is_empty(),
            "a pruned-anchor reservation must be released"
        );
        let unspent = store.get_unspent_notes(id).unwrap();
        assert_eq!(unspent.len(), 1, "the freed note is spendable again");
        assert_eq!(unspent[0].nullifier, nullifier);
    }

    /// A reserved note is excluded from `unspent_notes` (so a concurrent
    /// spend can't re-select it) and re-included once the reservation is
    /// cleared.
    #[test]
    fn unspent_notes_excludes_pending_and_reincludes_after_clear() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [0x55; 32];
        store
            .save_note(id, &note_with_nullifier(nullifier))
            .unwrap();
        assert_eq!(store.get_unspent_notes(id).unwrap().len(), 1);

        store.mark_pending(id, &nullifier).unwrap();
        assert!(
            store.get_unspent_notes(id).unwrap().is_empty(),
            "a reserved note is excluded from selection candidates"
        );

        assert!(store.clear_pending(id, &nullifier).unwrap());
        assert_eq!(
            store.get_unspent_notes(id).unwrap().len(),
            1,
            "releasing the reservation re-includes the note"
        );
    }

    /// `stale_pending_spends` ignores a reservation that was `mark_pending`ed
    /// but never armed with an anchor (a just-reserved, not-yet-built spend):
    /// the reconcile must never release such a transient entry.
    #[test]
    fn unarmed_reservation_is_not_stale() {
        let mut store = InMemoryShieldedStore::new();
        let id = test_id(0);
        let nullifier = [0x66; 32];
        store
            .save_note(id, &note_with_nullifier(nullifier))
            .unwrap();
        store.mark_pending(id, &nullifier).unwrap();

        assert!(
            store.stale_pending_spends(id).unwrap().is_empty(),
            "a reservation with no recorded anchor must not surface as stale"
        );
    }

    #[test]
    fn account_purge_retains_wallet_wide_identity_guard_only() {
        let wallet_id = [0x81; 32];
        let id = SubwalletId::new(wallet_id, 4);
        let mut store = InMemoryShieldedStore::new();
        let identity_guard = PendingRedrive {
            activity_id: [0x11; 32],
            anchor: [0x22; 32],
            nullifiers: vec![],
            st_bytes: vec![0x33; 16],
            attempts: 7,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
            identity_index: None,
        };
        let note_spend = PendingRedrive {
            activity_id: [0x44; 32],
            anchor: [0x55; 32],
            nullifiers: vec![[0x66; 32]],
            st_bytes: vec![0x77; 16],
            attempts: 1,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
            identity_index: None,
        };
        store.arm_redrive(id, identity_guard.clone()).unwrap();
        store.arm_redrive(id, note_spend).unwrap();

        store.purge_subwallet(id).unwrap();

        assert_eq!(
            store.pending_redrives(id).unwrap(),
            vec![identity_guard.clone()],
            "account removal must keep the unresolved identity debit but discard account spends"
        );
        assert_eq!(
            store.pending_redrives_for_wallet(wallet_id).unwrap(),
            vec![(id, identity_guard)],
            "the retained guard must remain visible to wallet-wide admission and redrive"
        );
    }

    // ---- claim-lease renewal (#4313 review finding 161a517fce36) ----

    fn claim_token(b: u8) -> AdmissionToken {
        AdmissionToken([b; 16])
    }

    /// THE BUG: without renewal a claim that outruns CLAIM_LEASE_MS is reaped,
    /// so a purge sees zero live claims and proceeds to destroy state the
    /// in-flight claim still needs. This is the negative control — it pins the
    /// behaviour the renewal exists to prevent.
    #[test]
    fn an_unrenewed_lease_lapses_and_stops_holding_off_a_purge() {
        let mut store = InMemoryShieldedStore::new();
        let wallet = [0xAA; 32];
        let token = claim_token(0x01);
        let t0 = 1_000_000u64;

        assert!(store
            .begin_claim_admission(wallet, token, t0, CLAIM_LEASE_MS)
            .unwrap());
        // Still inside the lease: a purge must WAIT.
        let live = store
            .begin_destructive_admission(None, claim_token(0x99), t0 + 1, DESTRUCTIVE_BARRIER_MS)
            .unwrap();
        assert_eq!(live, 1, "a live claim must hold off a purge");
        store.end_destructive_admission(claim_token(0x99)).unwrap();

        // One millisecond past the lease, with no renewal, the claim is invisible.
        let live_after = store
            .begin_destructive_admission(
                None,
                claim_token(0x98),
                t0 + CLAIM_LEASE_MS + 1,
                DESTRUCTIVE_BARRIER_MS,
            )
            .unwrap();
        assert_eq!(
            live_after, 0,
            "an unrenewed lease lapses — this is exactly what the heartbeat prevents"
        );
    }

    /// THE FIX: renewing keeps the claim counted well past the original lease.
    #[test]
    fn a_renewed_lease_keeps_holding_off_a_purge_past_the_original_window() {
        let mut store = InMemoryShieldedStore::new();
        let wallet = [0xAA; 32];
        let token = claim_token(0x02);
        let t0 = 1_000_000u64;

        assert!(store
            .begin_claim_admission(wallet, token, t0, CLAIM_LEASE_MS)
            .unwrap());

        // Three ticks at the heartbeat interval, as the claim path does.
        let tick = CLAIM_LEASE_MS / 3;
        let mut now = t0;
        for _ in 0..3 {
            now += tick;
            assert!(
                store
                    .renew_claim_admission(token, now, CLAIM_LEASE_MS)
                    .unwrap(),
                "renewal must succeed while the lease is live"
            );
        }

        // Past the ORIGINAL expiry, the claim is still counted.
        assert!(now > t0 + CLAIM_LEASE_MS - tick);
        let live = store
            .begin_destructive_admission(None, claim_token(0x97), now + 1, DESTRUCTIVE_BARRIER_MS)
            .unwrap();
        assert_eq!(live, 1, "a renewed claim must still hold off a purge");
    }

    /// Renewal must not RESURRECT a lease that already lapsed — a purge may
    /// already have counted it absent and acted on that.
    #[test]
    fn renewal_refuses_to_resurrect_a_lapsed_lease() {
        let mut store = InMemoryShieldedStore::new();
        let token = claim_token(0x03);
        let t0 = 1_000_000u64;
        assert!(store
            .begin_claim_admission([0xAA; 32], token, t0, CLAIM_LEASE_MS)
            .unwrap());
        assert!(
            !store
                .renew_claim_admission(token, t0 + CLAIM_LEASE_MS + 1, CLAIM_LEASE_MS)
                .unwrap(),
            "a lapsed lease must not come back"
        );
    }

    /// An unknown token renews nothing, rather than minting a lease.
    #[test]
    fn renewing_an_unknown_token_is_false_not_a_new_lease() {
        let mut store = InMemoryShieldedStore::new();
        assert!(!store
            .renew_claim_admission(claim_token(0x04), 1_000_000, CLAIM_LEASE_MS)
            .unwrap());
    }

    /// The in-memory store must give the SAME answer the file store's
    /// `INSERT ... ON CONFLICT DO NOTHING` gives — the wallet layer branches on
    /// this outcome and cannot tell the two backends apart (#4313
    /// cr-9d0e1a44).
    #[test]
    fn only_one_claimant_acquires_an_invitations_claim_key() {
        let mut store = InMemoryShieldedStore::new();
        let wallet: WalletId = [0xAA; 32];
        let key = [0xC1; 32];
        let id = SubwalletId::new(wallet, u32::MAX);
        let winner = claim_token(0x11);
        let loser = claim_token(0x12);
        let t0 = 1_000_000u64;

        // Both hold a per-wallet lease — that is not, and never was, mutual
        // exclusion between two claimants of one invitation.
        assert!(store
            .begin_claim_admission(wallet, winner, t0, CLAIM_LEASE_MS)
            .unwrap());
        assert!(store
            .begin_claim_admission(wallet, loser, t0, CLAIM_LEASE_MS)
            .unwrap());

        assert_eq!(
            store
                .reserve_one_time_claim_key(id, key, winner, t0, CLAIM_LEASE_MS)
                .unwrap()
                .reservation,
            ClaimKeyReservation::Acquired
        );
        assert_eq!(
            store
                .reserve_one_time_claim_key(id, key, loser, t0, CLAIM_LEASE_MS)
                .unwrap()
                .reservation,
            ClaimKeyReservation::Held {
                holder: winner,
                expires_at: t0 + CLAIM_LEASE_MS,
            }
        );

        // The gate refuses the loser's record write, so the winner's row cannot
        // be replaced.
        let record = |b: u8| PendingRedrive {
            activity_id: key,
            anchor: [b; 32],
            nullifiers: vec![[b; 32]],
            st_bytes: vec![b; 8],
            attempts: 0,
            identity_index: None,
            identity_nonce_finalized: false,
            identity_user_abandoned: false,
            anchor_height: None,
        };
        assert!(store
            .arm_redrive_under_claim(id, record(0xAA), winner, t0, CLAIM_LEASE_MS)
            .unwrap());
        assert!(
            !store
                .arm_redrive_under_claim(id, record(0xBB), loser, t0, CLAIM_LEASE_MS)
                .unwrap(),
            "a claimant without the key must not be able to write this record"
        );
        let records = store.pending_redrives(id).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].st_bytes, vec![0xAA; 8]);

        // Release hands the invitation straight over; no lease-length wait —
        // and the winner's durable record comes back WITH the reservation, so
        // the new owner resumes it instead of arming a second transition over
        // it (#4313 review finding r3767229122).
        store.end_claim_admission(winner).unwrap();
        let handover = store
            .reserve_one_time_claim_key(id, key, loser, t0, CLAIM_LEASE_MS)
            .unwrap();
        assert_eq!(handover.reservation, ClaimKeyReservation::Acquired);
        assert_eq!(
            handover.pending.as_ref().map(|r| r.st_bytes.clone()),
            Some(vec![0xAA; 8]),
            "acquiring a released key must hand back the record already armed under it"
        );
    }
}
