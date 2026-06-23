# DashPay Wallet ⟷ Platform-Wallet Integration (DashJ-backed)

**Status:** design / verified findings (no code yet).

**Context:** The DashPay Wallet (Android) keeps **DashJ** as its Core (L1) wallet —
HD key custody, SPV block sync, UTXO tracking, InstantSend / ChainLock receipt. We do
**not** want to replace that. We *do* want the Platform (L2) features that
`rs-platform-wallet` provides — DashPay social (contact requests, profiles), DPNS
usernames, identity management/update — layered on top, fed from DashJ.

This document records what was verified against the `rs-platform-wallet` /
`rs-platform-wallet-ffi` source, and the recommended architecture.

---

## TL;DR

- Run `rs-platform-wallet` **with its SPV off** (it's opt-in) and load wallets
  **watch-only** from DashJ's account xpubs. Signing is done by the external
  `KeystoreSigner` we already built. This is the intended shape of the FFI.
- **Don't** try to stream DashJ's live UTXOs into the platform wallet. The Core-state
  feed is a **one-time hydration**, not a live stream, and there is no incremental
  "inject a UTXO" FFI. You don't need it — see the split below.
- **Funding ops (register identity, top-up)** → use the **rs-sdk-ffi path already built**
  (`IdentityService.registerWithInstantLock`): DashJ produces the funded tx + InstantLock,
  we feed the proof in. No platform-wallet involvement, no UTXO-feed problem.
- **Non-funding Platform features (identity-update, DPNS, DashPay contacts/profile,
  documents)** → `rs-platform-wallet`, watch-only + signer. These never touch Core UTXOs,
  so the hydration limitation is irrelevant.

---

## Verified findings

### 1. SPV is opt-in; `manager_create` connects to nothing
`platform_wallet_manager_create` only builds the manager and spawns an event-adapter task
(`rs-platform-wallet-ffi/src/manager.rs`). The internal Core SPV client starts **only** via
`platform_wallet_manager_spv_start` (`spv.rs`); never calling it leaves Core SPV off.
(`spv.rs` note: SPV, when started, *must* run masternode sync so asset-lock proofs resolve
via CLSig/ISLock — which is exactly why we keep funding out of platform-wallet here.)

### 2. Wallets load **watch-only** from xpubs
`platform_wallet_manager_load_from_persistor` "reconstructs each wallet as **watch-only** via
its stored root + per-account xpubs, and registers them inside the manager"
(`manager.rs:295-303`). Private keys never enter platform-wallet; the external signer
(`org.dash.sdk.signing.KeystoreSigner`) provides signatures. This matches DashJ-holds-the-keys.

### 3. The restore structs are synthesizable from DashJ
`UtxoRestoreEntryFFI` (`wallet_restore_types.rs:306`) carries
`prev_txid, vout, value_duffs, script_pubkey(+len), height, is_coinbase, is_confirmed,
is_instantlocked, is_locked` plus an account-routing block
(`type_tag, standard_tag, account_index, registration_index, key_class,
user_identity_id[32], friend_identity_id[32]`). DashJ already tracks all of the UTXO fields;
the only mapping work is labelling each UTXO with its wallet account (both sides share the
BIP44 structure). `WalletRestoreEntryFFI` (`:395`) wraps these per wallet with `wallet_id`,
network, account xpubs (`AccountSpecFFI`), `identities`, sync heights, and tracked asset locks.

### 4. The Core-state feed is one-time hydration, **not** a live stream
- `platform_wallet_manager_load_from_persistor` is **not re-callable as a refresh**. Its own
  source comment (`rs-platform-wallet/src/manager/load.rs:49-59`): a re-run *"would collide on
  `WalletManager::insert_wallet` returning `WalletAlreadyExists` for every previously-loaded
  wallet"*. The transactional rollback there covers retry-after-failure, **not** refresh.
- There is **no** incremental `process_transaction` / `apply_transaction` / `import_utxo` FFI.
  Live Core updates are designed to come from the wallet's own SPV (`apply_chain_lock`, spv
  runtime) — which we're not using.
- The per-wallet `platform_wallet_load_and_apply_persisted` restores **only Platform addresses**
  (it discards `wallets: _`, see `platform_wallet.rs:1048-1062`), so it is *not* a Core-UTXO
  channel.
- Consequence: a watch-only wallet's UTXO view is effectively frozen at hydration. Refreshing
  it would mean tearing down and rebuilding the manager. **We avoid needing this entirely** by
  routing funding through rs-sdk-ffi (below).

### 5. Every callback field is individually nullable
`PersistenceCallbacks` / `EventHandlerCallbacks` fields are all `Option<fn>` (`persistence.rs`).
The two **struct pointers** must be non-null (`manager.rs` `check_ptr!`), but any individual
callback may be null — so we implement only the ones the watch-only social path needs.

---

## Recommended architecture

| Capability | Path | Why |
|---|---|---|
| Register identity / top-up | **rs-sdk-ffi** (built: `IdentityService.registerWithInstantLock` / `topUpWithInstantLock`) | DashJ supplies the funded tx + InstantLock; no UTXO feed needed |
| Identity update (add/disable keys) | `platform-wallet`, watch-only + signer | non-funding; rs-sdk-ffi has no identity-update entry point |
| DashPay contacts / profile | `platform-wallet`, watch-only + signer | the social layer; needs the wallet handle |
| DPNS username register | `platform-wallet`, watch-only + signer | non-funding |
| DPNS / identity / contract **reads** | **platform-sdk-jvm** (already built) | pure read-path |

The funding problem and the social-features problem are **disjoint**: the only thing that
needed a live Core view (funding) is handled by the rs-sdk-ffi proof-in path; everything we
want platform-wallet for is non-funding and runs on a once-hydrated watch-only wallet.

---

## Where it lands

`unified-sdk-jvm` (loads `librs_unified_sdk_ffi`, which is the only library that exports
`platform_wallet_*`). The read-path `platform-sdk-jvm` cannot resolve these symbols. The
existing `KeystoreSigner` / `SigningKeyStore` (in `platform-sdk-jvm`, rs-sdk-ffi-based) are
reused as the signer — they're already in the dependency floor of `unified-sdk-jvm`.

Setup chain:
1. Create a normal `DashSDK`; bind + call `dash_sdk_get_inner_sdk_ptr` for `sdk_ptr`.
2. `platform_wallet_manager_create(sdk_ptr, persistence, eventHandler, &out)` with the two
   vtable structs (most fields null — see below). **Do not** call `spv_start`.
3. `platform_wallet_manager_load_from_persistor` → fires `on_load_wallet_list_fn`; your impl
   returns the DashPay identity's wallet as a `WalletRestoreEntryFFI` (account xpubs +
   persisted identities). `platform_wallet_manager_get_wallet(wallet_id)` → wallet handle.
4. Drive the social/identity features with `*_with_signer` entry points + the `KeystoreSigner`.

---

## Persistence callbacks: what the watch-only social path actually needs

All 32 are `Option`, so unimplemented ones are passed null. For the **non-funding social/identity
layer with DashJ doing Core**, the working set is small. (Backing store = the app's Room DB.)

**Must implement (or nothing loads):**
- `on_load_wallet_list_fn` (+ `on_load_wallet_list_free_fn`) — hydrates the watch-only wallet
  (account xpubs + identities). The one hard requirement.

**Should implement (durability of what these features create):**
- `on_persist_identities_fn`, `on_persist_identity_keys_fn` — registered/updated identities + keys
- `on_persist_contacts_fn` — DashPay contacts
- `on_changeset_begin_fn` / `on_changeset_end_fn` / `on_store_fn` / `on_flush_fn` — the changeset
  transaction wrapper the persist callbacks fire inside (implement as a DB txn boundary)

**Can be null / no-op for this path:**
- All shielded callbacks (13): `on_persist_shielded_*`, `on_load_shielded_*` (+ frees) — only
  with the `shielded` feature/use case
- Funding-related: `on_get_core_tx_record_fn` (+ free), `on_persist_asset_locks_fn` — only if
  platform-wallet did funding, which it doesn't here
- Core-SPV-derived state we never populate (SPV off): `on_persist_address_balances_fn`,
  `on_persist_sync_state_fn`, `on_persist_account_registrations_fn`,
  `on_persist_account_address_pools_fn`, `on_persist_wallet_metadata_fn`,
  `on_persist_token_balances_fn`

> The exact functional-vs-optional status of the "should implement" set should be confirmed when
> building (the fields are structurally nullable, but skipping a persist means that data is
> in-memory only and won't survive an app restart — `on_load_wallet_list` can only return what was
> persisted). The shielded and funding nulls are safe given the use case.

`EventHandlerCallbacks` (6, all `Option`): wire `on_wallet_event_fn` + `on_error_fn` for
diagnostics; the `platform_address_sync_*` / `shielded_sync_*` events can be null.

---

## Hydration coordination contract (DashJ ⟷ platform-wallet)

- **Same derivation, shared `wallet_id`.** platform-wallet derives its watched addresses from
  the account xpubs DashJ supplies in `on_load_wallet_list_fn`. DashJ and platform-wallet must
  agree on `wallet_id` (32 bytes) and the per-account xpubs / BIP44 indices.
- **Identities ride on the wallet entry.** `WalletRestoreEntryFFI.identities`
  (`IdentityRestoreEntryFFI`) rehydrate the wallet's `IdentityManager` — the app persists these
  after a successful register/update and returns them on load.
- **Funding is external.** When the app wants to register/top-up, DashJ builds + funds the
  asset-lock tx and obtains its InstantLock; the app calls the **rs-sdk-ffi** register/top-up
  (proof in), not platform-wallet.

---

## Open items to confirm during implementation

1. Whether `manager_create` requires an ambient Tokio runtime on the calling thread (the Rust
   side enters the FFI's shared runtime in the constructor — verify the JNA call thread is fine).
2. Exact `@FieldOrder` layouts for `PersistenceCallbacks`, `EventHandlerCallbacks`,
   `WalletRestoreEntryFFI`, `UtxoRestoreEntryFFI`, `AccountSpecFFI`, `IdentityRestoreEntryFFI`
   → run through `jna-struct-auditor` (these are large, pointer-heavy, and array-bearing).
3. Whether the "should implement" persists are functionally required for the social ops to
   *complete* (vs only for durability).
4. How a *new* identity/wallet is added after initial hydration (load is one-time; adding a
   wallet may need `create_wallet_from_*` or a manager rebuild).
5. JNA callback GC-safety: the persistence/event vtables and their context pointers must be
   held for the manager's lifetime (same rule as `KeystoreSigner`'s callbacks).
