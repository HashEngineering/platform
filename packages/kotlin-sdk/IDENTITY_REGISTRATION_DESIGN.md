ok, # Identity Registration — Kotlin SDK Design Spike

**Status:** design-only (no code committed). Read-only spike against the
authoritative C headers under `native/include/` and the Swift precedent in
`packages/swift-sdk`.

**Scope:** how `register_identity` lands in the Kotlin SDK end-to-end — which
FFI entry point, the new callback surface, the `KeystoreSigner`, and the
unified-side `registerIdentity` wrapper.

**Lands in:** `unified-sdk-jvm` / `unified-sdk-android` (porting-plan Phase 6),
**not** `platform-sdk-jvm`. The flow needs `key-wallet-ffi` (mnemonic / funding)
alongside `platform-wallet-ffi`; `platform-sdk-jvm` stays pure-read.

---

## 1. FFI entry points

Three registration entry points exist in
`native/include/platform-wallet-ffi/platform-wallet-ffi.h`, all `*_with_signer`
(the mnemonic-driven `register_identity_from_addresses` was deleted). All three
share the output contract: write the 32-byte id into `out_identity_id[32]` and a
managed-identity `Handle` into `out_identity_handle`.

| Entry point | Funding source | Signers | Use case |
|---|---|---|---|
| `platform_wallet_register_identity_with_funding_signer` (h:3269) | **Builds a fresh asset lock** from wallet Core UTXOs (`amount_duffs`, `account_index`) | `SignerHandle` (identity) + `MnemonicResolverHandle` (core/funding) | **Primary one-shot.** "wallet funds → register." |
| `platform_wallet_resume_identity_with_existing_asset_lock_signer` (h:3300) | Existing tracked lock (`OutPointFFI`) | same two | Crash-recovery / "I already have an asset lock" |
| `platform_wallet_register_identity_with_signer` (h:3337) | Caller-supplied `IdentityFundingInputFFI[]` + `IdentityFundingOutputFFI` | **two `SignerHandle`s** (identity + address), no resolver | Low-level Platform-address funding |

```c
struct PlatformWalletFFIResult platform_wallet_register_identity_with_funding_signer(
    Handle wallet_handle,
    uint64_t amount_duffs, uint32_t account_index, uint32_t identity_index,
    const struct IdentityPubkeyFFI *identity_pubkeys, uintptr_t identity_pubkeys_count,
    SignerHandle *signer_handle,                 // identity-key signer
    MnemonicResolverHandle *core_signer_handle,  // funds the asset-lock credit-spend
    uint8_t (*out_identity_id)[32], Handle *out_identity_handle);
```

**Architectural point:** the whole "wallet funds asset lock → platform registers"
pipeline is a *single Rust entry point*. Kotlin never stitches it (SDK rule:
persist/load/bridge, no orchestration). The binding is a 1:1 wrapper.

Only `register_identity_with_signer` (no resolver) could in principle live in
`platform-sdk-jvm` — the "I already have funding credits + asset lock" escape
hatch — but it still needs a real `SignerHandle`, which is the new ground in §2.

---

## 2. The new problem: callbacks across JNA

Phase 1a is pure-read with zero callbacks. `register_identity` is the first time
Kotlin hands Rust a C function pointer that calls back into the JVM. Two callback
objects are required.

**(a) Signer** — `dash_sdk_signer_create_with_ctx(ctx, sign_async, can_sign, destroy)`
(rs-sdk-ffi.h:3603):
```c
typedef void (*SignAsyncCallback)(const void *signer, const uint8_t *pubkey_bytes, uintptr_t pubkey_len,
                                  uint8_t key_type, const uint8_t *data, uintptr_t data_len,
                                  void *completion_ctx, SignCompletionCallback completion);
typedef bool (*CanSignCallback)(const void *signer, const uint8_t *pubkey_bytes, uintptr_t pubkey_len, uint8_t key_type);
typedef void (*DestroyCallback)(void *signer);
void dash_sdk_sign_async_completion(void *completion_ctx, const uint8_t *signature, uintptr_t signature_len, const char *error_message);
```

**(b) Core-funding mnemonic resolver** — `dash_sdk_mnemonic_resolver_create(ctx, resolve, destroy)`
(rs-sdk-ffi.h:3362):
```c
typedef int32_t (*MnemonicResolveCallback)(const void *ctx, const uint8_t *wallet_id_bytes,
                                           char *out_mnemonic_utf8, uintptr_t out_capacity, uintptr_t *out_len);
```

### JNA-specific notes (these differ from Swift's `Unmanaged` dance)

1. **No `_with_ctx` gymnastics.** Swift passes `ctx = Unmanaged.passUnretained(self)`
   because C function pointers can't capture. JNA `Callback` objects *are* objects
   and close over Kotlin state directly — so prefer **`dash_sdk_signer_create`
   (no ctx)** and ignore `ctx`. The Swift `passRetained` leak saga does not transfer.
2. **The real footgun is GC.** JNA generates one native trampoline per `Callback`
   instance and keeps it alive only while a strong reference exists. If the
   `Callback` is collected while Rust still holds the pointer → hard crash.
   `KeystoreSigner` must hold its three `Callback`s as `private val` fields for the
   whole `SignerHandle` lifetime, and `dash_sdk_signer_destroy` only after the call.
3. **Threading.** `SignAsyncCallback` may fire from any Tokio worker thread. JNA
   auto-attaches a daemon JVM thread, but pin it explicitly with a named
   daemon `CallbackThreadInitializer` so failures aren't silent on a detached thread.
4. **Synchronous completion is fine** (mirrors Swift v1): v1 signing is fast, so
   compute the signature inside `sign_async` and call
   `dash_sdk_sign_async_completion(completion_ctx, …)` before returning. The async
   model exists for slow biometric prompts; not needed yet.

---

## 3. `KeystoreSigner` (mirror of Swift `KeychainSigner`)

Job: given raw pubkey bytes + a `key_type` discriminant, find the matching private
key and produce a secp256k1 signature over `data`. Swift dispatches on `key_type`:
- `key_type < 5` → identity key: look up persisted private key by pubkey, sign.
- `key_type == 0xFF` → platform-address: derive-and-sign on demand. **Not needed
  for the funding-signer path** (that branch belongs to the low-level / address flow).

Kotlin v1 mirror (illustrative — not committed):

```kotlin
class KeystoreSigner(
    private val keyStore: IdentityKeyStore,   // abstract; backing decided later
    private val network: Network,
) : AutoCloseable {

    // Held as fields so JNA does not GC the native trampolines.
    private val signCb = DashSdkFfi.SignAsyncCallback { _, pubkey, pubkeyLen, _, data, dataLen, completionCtx, _ ->
        val pub = pubkey?.getByteArray(0, pubkeyLen.toInt()) ?: ByteArray(0)
        val msg = data?.getByteArray(0, dataLen.toInt()) ?: ByteArray(0)
        try {
            val priv = keyStore.privateKeyFor(pub) ?: error("no key for pubkey")
            val sig = ffiSignOnce(priv, msg)   // dash_sdk_signer_create_from_private_key round-trip (Swift v1 parity)
            Memory(sig.size.toLong()).let { m ->
                m.write(0, sig, 0, sig.size)
                ffi.dash_sdk_sign_async_completion(completionCtx, m, NativeLong(sig.size.toLong()), null)
            }
        } catch (e: Throwable) {
            ffi.dash_sdk_sign_async_completion(completionCtx, null, NativeLong(0), e.message ?: "sign failed")
        }
    }
    private val canSignCb = DashSdkFfi.CanSignCallback { _, pubkey, len, _ ->
        keyStore.hasKeyFor(pubkey?.getByteArray(0, len.toInt()) ?: ByteArray(0))
    }
    private val destroyCb = DashSdkFfi.DestroyCallback { /* no-op; state is GC-managed */ }

    val handle: Pointer = ffi.dash_sdk_signer_create(signCb, canSignCb, destroyCb)
        ?: error("dash_sdk_signer_create returned NULL")

    override fun close() = ffi.dash_sdk_signer_destroy(handle)
}
```

### Key storage — DEFERRED (decided later)

`IdentityKeyStore` stays an abstract interface in this design; the concrete
backing is intentionally unresolved. Constraints that will shape it:

- There is **no secp256k1 hardware path on the JVM/Android.** Android Keystore is
  NIST-curve only — it can neither hold nor sign a secp256k1 key. So the 32-byte
  scalar is app-managed encrypted material in every flavor, and v1 signing rounds
  through `dash_sdk_signer_create_from_private_key` → `dash_sdk_signer_sign` →
  `destroy` (exactly Swift v1).
- Two backing options to weigh at implementation time:
  - **Rust-side persist callback** — `dash_sdk_derive_and_persist_identity_keys`
    (h:3051) with a `PersistKeyCallback` (`PersistKeyArgs`, h:297). Raw scalars are
    handed to Kotlin only transiently per key; never round-tripped through the
    register call. Most secure; adds a third callback to bind.
  - **App-managed encrypted store** — Jetpack Security (`EncryptedFile` /
    `EncryptedSharedPreferences`, AES-GCM + Android-Keystore-wrapped master key) on
    android; `KeyStore`-file or in-memory on jvm. Direct Swift v1 parity.

---

## 4. Unified `registerIdentity` wrapper (mirror of `registerIdentityWithFunding`)

Swift orchestration: `ManagedPlatformWallet.swift:2708`. Reduces to: pin the
pubkey rows, keep both signers alive across the call, invoke the one FFI, guard
the NULL handle. Kotlin equivalent (illustrative — not committed):

```kotlin
fun registerIdentity(
    amountDuffs: ULong, accountIndex: UInt, identityIndex: UInt,
    identityPubkeys: List<IdentityPubkey>,   // pre-derived via dash_sdk_derive_identity_keys_from_mnemonic
    identitySigner: KeystoreSigner,          // signs the IdentityCreate transition
    coreResolver: MnemonicResolver,          // funds the asset-lock credit-spend
): RegisteredIdentity {
    require(identityPubkeys.isNotEmpty())

    val pinned = identityPubkeys.map { it.pubkeyBytes.toPinnedMemory() }   // hold refs until after the call
    val rows = IdentityPubkeyFFINative().toArray(identityPubkeys.size) as Array<IdentityPubkeyFFINative>
    identityPubkeys.forEachIndexed { i, pk -> rows[i].fill(pk, pinned[i]) }

    val outId = Memory(32)
    val outHandle = PointerByReference()

    val res = ffi.platform_wallet_register_identity_with_funding_signer(
        walletHandle, amountDuffs.toLong(), accountIndex.toInt(), identityIndex.toInt(),
        rows[0].pointer, NativeLong(rows.size.toLong()),
        identitySigner.handle, coreResolver.handle,
        outId, outHandle,
    )
    res.checkSuccess()                         // existing ResultUnwrapper convention
    val managed = outHandle.value ?: error("success but NULL identity handle")  // Swift's same guard
    return RegisteredIdentity(id = outId.getByteArray(0, 32), handle = managed)
}
```

**Preconditions the wrapper must document** (same as Swift): caller pre-derives
keys (`dash_sdk_derive_identity_keys_from_mnemonic`, h:3173) and pre-persists each
private scalar where the `KeystoreSigner` can find it mid-registration — unless the
Rust-side persist-callback option is chosen, which folds derive+persist into one call.

---

## 5. Binding work-list (Phase 6, when greenlit)

**`DashSdkFfi.kt` additions (unified flavor):**
- `dash_sdk_signer_create` / `dash_sdk_signer_create_with_ctx` / `dash_sdk_signer_destroy`
- `dash_sdk_sign_async_completion`
- `dash_sdk_signer_create_from_private_key` / `dash_sdk_signer_sign` / `dash_sdk_signature_free` (v1 sign primitive)
- `dash_sdk_mnemonic_resolver_create` / `dash_sdk_mnemonic_resolver_destroy`
- `platform_wallet_register_identity_with_funding_signer`
- `dash_sdk_derive_identity_keys_from_mnemonic` (+ `_free`)
- Callback interfaces: `SignAsyncCallback`, `CanSignCallback`, `DestroyCallback`,
  `MnemonicResolveCallback` (+ `PersistKeyCallback` if the persist-callback storage option wins)

**New JNA `Structure`s — must go through `jna-struct-auditor`:**
- `IdentityPubkeyFFINative` (12 fields, `@FieldOrder`; three borrowed pointers +
  inline discriminant — exactly the layout shape that SIGSEGVs if misaligned)
- `IdentityRegistrationKeyDerivationsFFI` / `IdentityKeyPreviewFFI` if the derivation
  path is surfaced
- `PersistKeyArgs` (h:297) if the persist-callback option wins

**New Kotlin classes:** `KeystoreSigner`, `MnemonicResolver`, `IdentityKeyStore`
(abstract; android/jvm impls deferred), `IdentityPubkey` model, `RegisteredIdentity`.

**Escape hatch (Level-1):** `platform_wallet_resume_identity_with_existing_asset_lock_signer`
for the "I already have an asset lock" recovery surface — same signer + resolver,
takes an `OutPointFFI` instead of `amount_duffs`/`account_index`.
