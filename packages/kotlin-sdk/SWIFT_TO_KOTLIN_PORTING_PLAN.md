# Swift SDK → Kotlin SDK Porting Plan

Status: **Draft / proposal** · Target package: `packages/kotlin-sdk` · Source of truth: `packages/swift-sdk`

This document is the plan to translate the Swift Dash Platform SDK (`packages/swift-sdk`)
into the Kotlin SDK (`packages/kotlin-sdk`): the SDK source, an example app, and the test
suites. It is intended to be read top-to-bottom by a contributor (human or AI agent) who
will pick up phases incrementally.

---

## 1. Goal & scope

Port three things from `swift-sdk` to `kotlin-sdk`, preserving behavior and the
architectural contract:

1. **SDK source** — `Sources/SwiftDashSDK/` (112 files, ~42k LOC) → Kotlin library modules.
2. **Example app** — `SwiftExampleApp/` (138 SwiftUI files) → an Android (Jetpack Compose) example app.
3. **Tests** — `SwiftTests/SwiftDashSDKTests` (~206) + `SwiftExampleAppTests` (~79) + UI tests → JVM/Android JUnit + instrumented + Compose UI tests.

**Out of scope:** changing the Rust FFI contract beyond what's needed to expose the same
entry points the Swift SDK already consumes. Business logic stays in Rust (see §3).

---

## 2. Current state (gap analysis)

### What exists in `kotlin-sdk` today
A thin, **read-only** JNA-based SDK. Working, but a small fraction of the Swift surface.

The SDK ships in two compile-time flavors — **platform** (`rs-sdk-ffi`, read-path) and
**unified** (`rs-unified-sdk-ffi`, full SDK + wallet + shielded). The unified native
library is a superset of the read-path symbols, so the unified flavor builds on the
platform flavor (no separate shared module).

| Module | Purpose | Target |
|---|---|---|
| `platform-sdk-jvm` | Read-path SDK (pure JVM): FFI bindings + services + models + tests; defaults to `rs_sdk_ffi` | JVM 17 |
| `platform-sdk-android` | Android library: `api(:platform-sdk-jvm)` + `librs_sdk_ffi.so` | Android API 24+ |
| `unified-sdk-jvm` | Unified flavor: `api(:platform-sdk-jvm)` + pins `rs_unified_sdk_ffi`; home of net-new wallet/shielded bindings | JVM 17 |
| `unified-sdk-android` | Android library: `api(:unified-sdk-jvm)` + `librs_unified_sdk_ffi.so` | Android API 24+ |
| `console` | CLI DPNS-search demo (depends on `:platform-sdk-jvm`) | JVM 17 |

Implemented (in `platform-sdk-jvm`): `DashSDK` facade (`create`/`close`), JNA bindings (`DashSdkFfi`,
`NativeLoader`, `NativeLibrary`, `ResultUnwrapper`), models (`Identity`, `DataContract`,
`Document`, `Network`, `DashSDKException`), read services (`IdentityService`,
`DataContractService`, `DocumentService`, `DpnsService`). All `suspend` on
`Dispatchers.IO`. JSON parsed via regex.

> The net-new wallet/shielded work below lands in `:unified-sdk-jvm` (it needs symbols
> that exist only in `rs-unified-sdk-ffi`); read-path additions go in `:platform-sdk-jvm`.

Tooling: JNA 5.14, Kotlin 2.0.21, coroutines 1.9, Gradle 8.9, AGP 8.5.2. Native build via
`build_native.sh` and the per-flavor wrappers (`build_{platform,unified}_{local,android}.sh`).

### What the Swift SDK has that Kotlin does not (the work)
- **KeyWallet** (27 files): BIP-39 mnemonics, HD wallets, accounts (ECDSA/BLS/EdDSA), key
  derivation, addresses + gap-limit pools, transactions/UTXOs, managers.
- **PlatformWallet** (19+ files): the `PlatformWalletManager` coordinator, SPV sync, BLAST
  platform-address sync, shielded (Orchard) sync + funding, identity discovery/registration,
  DashPay (profiles, contact requests), tokens, asset-lock, persistence callbacks.
- **Persistence** (29 `@Model` SwiftData entities + container + migration plan).
- **Security** (Keychain manager + inspector) and **FFI callback bridges** (signer,
  mnemonic resolver, persister).
- **DPP types** (Identity/Document/DataContract/StateTransition + key enums).
- **Address, Utils, Helpers, Wallet, Config, Services, Core** (logging, contract parser).
- Write/broadcast operations, signing, streaming sync — none exist in Kotlin yet.

**Bottom line:** the existing Kotlin code is a usable read-path foundation. The port is
~90% net-new code, organized to match the Swift module layout.

---

## 3. Guiding principle (do not violate)

`swift-sdk/CLAUDE.md` defines a hard architectural rule that **carries over verbatim** to Kotlin:

> The SDK does exactly three things: **persist** data, **load** data, **bridge** FFI.
> The FFI crates do exactly one thing: expose existing Rust functions over a C ABI.

For the Kotlin port this means:
- **No business logic in Kotlin.** No gap-limit walks, no derivation-path building, no
  multi-step pipelines (mnemonic → seed → path → key → store), no re-implemented protocol
  constants. All of that lives in `platform-wallet` / `key-wallet` / `rs-sdk` Rust crates and
  is reached through a single FFI entry point.
- **The only Kotlin-owned decision is the Keystore write** — the Android analogue of the iOS
  Keychain exception: accept `(path_string, 32 private-key bytes)` from Rust and persist.
- Every Swift file that "decides something" should already be calling one FFI function; the
  Kotlin file mirrors that 1:1. If you find yourself porting a loop or a policy, stop — it
  belongs in Rust.

A `kotlin-sdk/CLAUDE.md` should be created restating this rule (Phase 0).

---

## 4. Technology mapping

| Swift / Apple | Kotlin / Android | Notes |
|---|---|---|
| Swift Package (`Sources/SwiftDashSDK`) | Gradle modules `sdk-jvm` + `sdk` | Keep the JVM/Android split already in place |
| `DashSDKFFI` (C interop) | JNA bindings (`DashSdkFfi`) | Existing approach. Callback-heavy code (signer/persister/resolver) → JNA `Callback`. Consider JNI/Panama only if profiling demands it (§6) |
| iOS Keychain | Android Keystore + `EncryptedSharedPreferences` / Tink | Biometric gate via `BiometricPrompt` |
| SwiftData `@Model` (29) | Room `@Entity` + DAO + `RoomDatabase` | `#Predicate` → Room `@Query`; cascade/nullify → Room FK `onDelete` |
| `ModelContainer` + `DashMigrationPlan` | `RoomDatabase` + `Migration` objects | Additive migrations |
| `@Published` / `ObservableObject` / Combine | `StateFlow` / `SharedFlow` in a holder/VM | |
| `async/await`, `@MainActor`, `Task.detached` | coroutines, `Dispatchers.Main`, `launch`/`async` | Keep callbacks off the main thread |
| `@unchecked Sendable` handle wrappers | plain classes; guard shared mutable state w/ `Mutex`/atomics | Rust side is thread-safe |
| `Codable` + `CodingKeys` | `kotlinx.serialization` `@Serializable` + `@SerialName` | Replaces the current regex JSON parsing |
| enum w/ associated values | `sealed class` / `sealed interface` | `SDKError`, sync events, result types |
| `DispatchSemaphore` (sync callback bridge) | `CountDownLatch` / `runBlocking` channel | For synchronous FFI callbacks |
| `OpaquePointer` / `UnsafeMutablePointer<Handle>` | JNA `Pointer` / `Long` handle | Free in `close()`/`AutoCloseable`, not finalizers |
| **SwiftUI** (`SwiftExampleApp`) | **Jetpack Compose** | `@EnvironmentObject` → Hilt or `CompositionLocal`; `NavigationStack` → Navigation-Compose; `TabView` → `NavigationBar`; `@Query` → `collectAsState()` over Room Flows |
| `LAContext` biometrics | `androidx.biometric.BiometricPrompt` | |
| `AVCaptureSession` QR scan | CameraX + ML Kit barcode | Example app only |
| XCTest / Swift Testing | JUnit5 (or 4) + kotlin.test; Compose UI test + instrumented | |

---

## 5. Target module structure

Keep the existing 3-module layout and grow it; add an example app module.

```
kotlin-sdk/
├── sdk-jvm/      # pure-JVM: FFI bindings, DPP models, services, wallet logic-free wrappers
│   └── org.dash.sdk/
│       ├── ffi/            # JNA bindings + callbacks (signer, persister, resolver)
│       ├── models / dpp/   # DPP types, enums, @Serializable data classes
│       ├── keywallet/      # mnemonic, wallet, account, address, tx wrappers
│       ├── platformwallet/ # manager, sync wrappers, dashpay, tokens, assetlock
│       ├── address/, utils/, helpers/, config/, core/ (logging)
│       └── DashSDK.kt      # facade (already present, to be expanded)
├── sdk/         # Android library: depends on :sdk-jvm
│   └── org.dash.sdk.android/
│       ├── persistence/    # Room entities (29), DAOs, database, migrations
│       ├── security/       # Keystore signer, biometric, keystore-backed resolver/persister
│       └── state/          # StateFlow holders mirroring PlatformWalletManager observables
├── example-app/ # NEW Android app module (Compose) ≈ SwiftExampleApp
├── console/     # existing CLI (extend to mirror example flows where headless-feasible)
└── build_android.sh / build_local.sh  # extend to build the FULL native surface (§6)
```

Persistence + Keystore live in the **Android** module (Room/Keystore are Android APIs).
`sdk-jvm` should define persistence/security as **interfaces** so JVM consumers (and tests)
can supply non-Android implementations; the Android module provides Room/Keystore impls.
This mirrors the Swift design where the SDK is handed a persistence + keychain handler.

---

## 6. Native / FFI strategy — RESOLVED & BUILD-PROVEN by Phase-0 spike

The spike (2026-06-19) settled this **and proved the build end-to-end**. Far simpler than
feared: **the unified crate already exists and bundles everything.**

### Build proof (arm64-v8a)
`./build_android.sh arm64 --unified` succeeded (exit 0, 419 crates, 3m49s, NDK 29.0.14206865):
- Produced `librs_unified_sdk_ffi.so` (77 MB, ELF aarch64), copied to
  `sdk/src/main/jniLibs/arm64-v8a/`, **alongside** the preserved `librs_sdk_ffi.so` (52 MB).
- 750 exported `T` functions: **226** `dash_sdk_*`, **170** `platform_wallet_*`, plus the
  key-wallet surface (prefix note below) and **23** shielded/Orchard symbols
  (e.g. `platform_wallet_manager_bind_shielded`) — confirming `--features shielded` works.
- cbindgen headers for all four crates auto-collected into `kotlin-sdk/native/include/`.
- transitive C deps (openssl-sys, etc.) cross-compiled cleanly — the one remaining risk is gone.
- **Prefix note for binding work:** `key-wallet-ffi` exports under *domain* prefixes
  (`managed_account_`, `account_collection_`, `wallet_manager_`, `managed_wallet_`,
  `eddsa_account_`, `bls_account_`, `core_wallet_`, `asset_lock_`, …), **not** `key_wallet_`
  (131 functions in its header). Binding generation must key off the headers, not one prefix.

The build script now takes `--unified` (default stays `rs-sdk-ffi`, so the read path is
untouched; both `.so`s coexist in `jniLibs/`).

### ABI decision — 64-bit only (arm64-v8a + x86_64)
Building the unified lib for **x86_64** also succeeded (226/170/23, identical surface).
Building for **armeabi-v7a** (32-bit ARM) **failed**: `platform-wallet-ffi` has hard-coded
compile-time struct size/alignment guards written for 64-bit pointer width
(`const _: [u8; 144] = [0u8; size_of::<ContactRequestFFI>()]` etc.; 7 such guards across
`contact_persistence`, `derive_and_persist_callbacks`, `identity_persistence`,
`wallet_registration_persistence`). On 32-bit, pointers/usize shrink 8→4 bytes so the structs
are smaller and the guards fail to compile. **x86 (i686) is also 32-bit and fails identically.**
iOS only ever targeted aarch64, so 32-bit was never validated upstream.

**Decision (confirmed with user):** ship the unified library for the two **64-bit** ABIs only —
`arm64-v8a` (devices) + `x86_64` (emulator) — matching iOS and modern Android (Google Play has
mandated 64-bit since 2019). Making 32-bit work would require `cfg(target_pointer_width)`
branching on every guard *and* on the Kotlin/JNA struct layouts — touching the Rust FFI
contract (out of scope per §1) for a legacy ABI. Applied: `build_android.sh` (`all` = arm64 +
x86_64; 32-bit cases removed) and `sdk/build.gradle.kts` `abiFilters`; stale 32-bit jniLibs
dirs removed.

### Phase 0 — DONE
- Android unified `.so` for **arm64-v8a** + **x86_64** (both 226/170/23) in `sdk/src/main/jniLibs/`.
- Host unified dylib via `./build_local.sh --unified` → `<workspace>/target/release/librs_unified_sdk_ffi.dylib`
  (62 MB Mach-O arm64, 226/170/23). Cargo workspace shares one `target/` at the **repo root**,
  not under `packages/kotlin-sdk/` — Gradle's JVM-test config already points there.
- `--unified` wired in `build_android.sh`, `build_local.sh`, `build_local.bat` (default stays
  `rs-sdk-ffi`). Library selection at runtime via `dash.sdk.native.lib` /
  Gradle `-PdashNativeLib` → `NativeLibrary` → loader + JNA.
- cbindgen headers collected to `kotlin-sdk/native/include/{dash-network,key-wallet-ffi,
  rs-sdk-ffi,platform-wallet-ffi}/*.h` — the input for Phase 1 binding generation.
- Hygiene: `.gitignore` now excludes `bin/` and `librs_unified_sdk_ffi.so`; the wrongly-committed
  IDE `bin/` mirror dirs (`sdk-jvm/bin`, `console/bin`) untracked.

**Phase 1 (generate JNA bindings from `native/include/*.h`) is unblocked.**

### Findings
- **`packages/rs-unified-sdk-ffi`** is the unified library. Its `Cargo.toml` declares
  `crate-type = ["staticlib", "cdylib"]` (iOS `.a` + Android `.so`) and depends on
  `key-wallet-ffi`, `platform-wallet-ffi` (`../rs-platform-wallet-ffi`), `rs-sdk-ffi`, and
  `dash-network` (ffi). Its `src/lib.rs` is four `pub use` lines re-exporting them. Output
  artifact: **`librs_unified_sdk_ffi.{a,so}`**.
- **`key-wallet-ffi` is a Cargo git dependency** (`rust-dashcore`, rev `5c0113e7…`), pulled in
  automatically — **no separate checkout needed** (this retires the survey's "not found" risk).
  There is no separate `dash-spv-ffi`; SPV/core lives inside `platform-wallet-ffi`.
- **Headers are generated automatically on every `cargo build`.** Each FFI crate's `build.rs`
  runs cbindgen (config in each crate's `cbindgen.toml`) and writes
  `target/<target>/<profile>/include/<crate>/<crate>.h` — confirmed present already for
  `platform-wallet-ffi`. So a normal Android/host build also produces the headers we'll
  generate JNA bindings from. No iOS-only step required.
- **iOS recipe** (`swift-sdk/build_ios.sh`): `cargo build -p rs-unified-sdk-ffi --features
  shielded [--features tokio-metrics on dev]` per Apple target, then it merges four headers —
  **in this order** (earlier define types later ones reference): `dash-network`,
  `key-wallet-ffi`, `rs-sdk-ffi`, `platform-wallet-ffi` — into an umbrella `DashSDKFFI.h`,
  gives opaque forward-declared structs a `{ uint8_t _opaque; }` body, and packages an
  xcframework. Android needs the build + header steps, not the xcframework packaging.
- **`shielded` (Orchard) is a Cargo feature**, opt-in at crate level, on by default for the iOS
  framework. Android must pass `--features shielded` for shielded-pool parity.

### Decisions
1. **One merged cdylib.** Build `rs-unified-sdk-ffi` → bundle a single
   `librs_unified_sdk_ffi.so` per ABI (Android analogue of `DashSDKFFI.xcframework`). Kotlin
   binds one library.
2. **Generate JNA bindings from the four cbindgen headers** (in the merge order above) rather
   than hand-writing per-crate interfaces. The existing hand-written `DashSdkFfi.kt` (254 LOC,
   read path) becomes the seed/spec for the generator.
3. **Callbacks** (signer / mnemonic resolver / persister) are Rust→Kotlin: use JNA `Callback`,
   and **pin** callback objects (strong ref) for as long as Rust holds them; run the persister
   off the main thread. Fall back to a JNI shim only if JNA proves fragile under profiling.

### Concrete change to the Android build (small, well-scoped)
In `kotlin-sdk/build_android.sh`:
- `FFI_PACKAGE_DIR` → `rs-unified-sdk-ffi`; build flags `--package rs-sdk-ffi` →
  `-p rs-unified-sdk-ffi --features shielded`.
- Copy `target/<rust-target>/release/librs_unified_sdk_ffi.so` →
  `jniLibs/<abi>/librs_unified_sdk_ffi.so` (was `librs_sdk_ffi.so`).
- Update `NativeLoader` to load `rs_unified_sdk_ffi`; update `--clean` target list package.
- After a build, copy headers from `target/<rust-target>/release/include/*/*.h` for the
  binding generator.
- Mirror the same package swap in `build_local.sh` (host `.dylib`/`.so` for JVM tests).

Deliverable: updated `build_android.sh` / `build_local.sh` producing the unified `.so` for 4
ABIs + host, and a Kotlin `BUILD_GUIDE_FOR_AI.md`.

---

## 7. Phased work breakdown

Each phase is independently reviewable and leaves the tree buildable. Phases 1–7 are the SDK;
8 the app; 9 tests; 10 polish. Within the SDK, order is chosen so each layer compiles against
the one below.

### Phase 0 — Foundations & native surface  *(blocking)*
- Settle the unified native library strategy (§6); extend build scripts; bundle/produce all
  required `.so`s for 4 ABIs and host (`.dylib`/`.so`).
- Generate or scaffold full JNA bindings for the merged header (`dash_sdk_*`,
  `platform_wallet_*`, `key_wallet_*`, `dash_core_sdk_*`).
- Add `kotlinx.serialization`, Room, `androidx.biometric`, Compose, Hilt (or chosen DI),
  CameraX/ML-Kit to `libs.versions.toml`.
- Write `kotlin-sdk/CLAUDE.md` (architectural rule, §3) and a Kotlin `BUILD_GUIDE_FOR_AI.md`.

### Phase 1 — FFI bindings & result/error infrastructure
Swift refs: `FFI/`, `SwiftDashSDK.swift`, `PlatformWalletFFI.swift`, `ResultUnwrapper`.
- Full JNA `interface`s + `Structure`s for all entry points and `repr(C)` structs.
- `ResultUnwrapper` parity for the new result types; `SDKError` → Kotlin `sealed class`.
- Callback scaffolding (typed JNA `Callback` wrappers) for signer/persister/resolver — impls
  land in later phases, but the marshalling contract is fixed here.

### Phase 2 — DPP & domain models
Swift refs: `DPP/`, `Models/`, `DashNetwork.swift`, `Address/PlatformAddressInfo`, `Wallet/WalletModels`, `Utils/DataExtensions` (base58/hex), `Validation`.
- `@Serializable` data classes + enums (`KeyType`, `KeyPurpose`, `SecurityLevel`, `Network`
  already exists, extend).
- Replace regex JSON with `kotlinx.serialization`. Port base58/hex codecs and validators.
- State-transition form definitions (`StateTransitionDefinitions`) as data + sealed inputs.

### Phase 3 — Persistence (Room)
Swift refs: `Persistence/` (29 `@Model`), `DashModelContainer`, `DataContractParser`, `ContractIdentityLinker`, `Services/DataManager`.
- One Room `@Entity` per `PersistentX`; relationships via FK + `@Relation`; uniqueness via
  indices; cascade/nullify via `onDelete`.
- `RoomDatabase` + `Migration`s mirroring `DashMigrationPlan` (additive).
- DAOs exposing `Flow<…>` for reactive reads (the `@Query` analogue).
- Port `DataContractParser` (it parses Rust JSON → entities — this is marshalling, allowed).
- Define a `PersistenceHandler` interface in `sdk-jvm`; Room impl in `sdk` (Android).

### Phase 4 — Security & callback bridges
Swift refs: `Security/` (`KeychainManager`, `KeychainInspector`), `FFI/KeychainSigner`, `FFI/MnemonicResolverAndPersister`, `Core/Wallet/WalletStorage`.
- Android Keystore-backed `KeystoreManager` (store/retrieve private key bytes, special MN keys).
- Biometric gate via `BiometricPrompt`.
- `KeystoreSigner` implementing the Rust signer callback (locate pubkey row → fetch scalar →
  `dash_sdk_signer_*` → zero buffer). Port the masked-mnemonic resolver and the key persister
  callback. **This is the one place Kotlin "owns" a step** (Keystore write).
- Mnemonic/metadata storage (`WalletStorage` analogue) in `EncryptedSharedPreferences`.

### Phase 5 — KeyWallet wrappers
Swift refs: `KeyWallet/` (27 files).
- Thin handle-wrapper classes: `Mnemonic`, `Wallet`, `Account`(+BLS/EdDSA), `KeyDerivation`,
  `Address`/`AddressPool`, `Transaction`/`TxOutput`, `KeyManager`/`WalletManager`,
  `Managed*` lifecycle wrappers, `KeyWalletTypes` enums.
- Every method = one `key_wallet_*` FFI call. No derivation logic in Kotlin.

### Phase 6 — PlatformWallet & sync
Swift refs: `PlatformWallet/` (19+ files incl. `*SPV`, `*AddressSync`, `*IdentitySync`, `*ShieldedSync`, `*ShieldedFunding`, `IdentityManager`, `Managed*`, `DashPay*`, `Tokens/`, `AssetLock/`, `CoreWallet/`, `PlatformWalletPersistenceHandler`).
- `PlatformWalletManager` as a coroutine-based coordinator exposing `StateFlow`s mirroring the
  Swift `@Published` set (spvProgress, syncing flags, wallets map, lastError, shielded tree
  progress, etc.).
- Sync orchestration = polling/event tasks driven by FFI; the persister callback fans changes
  into Room (Phase 3) and Keystore (Phase 4).
- DashPay models, token actions, asset-lock manager wrappers.

### Phase 7 — High-level SDK API & services
Swift refs: `SDK.swift` (entry point, nested `Identities`/`Addresses`/`SDKStatus`), `Core/Services/SDKLogger`, `Address/Addresses`, existing `services/`.
- Expand the existing `DashSDK` facade: init/version negotiation/DAPI discovery/quorum config,
  protocol-version refresh, known-contract preload.
- Keep/extend existing read services; add identity-balance batch, address-info queries.
- `SDKLogger` (preset levels, file logging to app cache dir).

### Phase 8 — Example app (Android / Compose)
Swift ref: `SwiftExampleApp/` (138 files, 5 tabs).
- New `example-app` module. Architecture: Compose + Navigation + ViewModels(`StateFlow`) +
  Hilt + Room (reuse `sdk` module). Mirror the 5 tabs:
  1. **Sync status** (SPV heights, BLAST sync, shielded stats)
  2. **Wallets** (create/import/restore, accounts, addresses, send/receive, tx history, QR)
  3. **Identities** (create/register/top-up, keys, DPNS, DashPay contacts)
  4. **Contracts** (download/explore contracts & document types, documents, **token actions** —
     mint/burn/freeze/transfer/pause/set-price/claim/purchase + co-sign/group actions)
  5. **Settings** (network switch, diagnostics, query builder, storage/keystore explorers)
- Network-locked manager design (one manager per network, cached) → a per-network DI scope or
  keyed map, mirroring `WalletManagerStore`.
- Port pure helpers first (QR payload parser, recipient validators, resumable-registration
  filter, pasteboard-contract candidate) — they're unit-testable and feed Phase 9.

### Phase 9 — Tests
Swift refs: `SwiftTests/SwiftDashSDKTests` (~206), `SwiftExampleAppTests` (~79), UI tests.
- **JVM unit tests** (`sdk-jvm`): data transformers, error mapping, validation, base58/hex,
  key/identity type conversions, SDK-method bindings against a mock SDK (no DAPI) — mirror
  `DataTransformersTests`, `ErrorHandlingTests`, `ValidationTests`, `SDKMethodTests`,
  `IdentityManagerTests`, `KeyManagerTests`, `PlatformWalletTypesTests`.
- **Instrumented/integration tests** (`sdk`, Android): native lib load, create/destroy,
  Room persistence round-trips, Keystore signer, multi-network isolation, platform-wallet
  integration — mirror `PlatformWalletTests`, `PlatformWalletIntegrationTests`,
  `ManagedPlatformAddressWalletTests`, `ShieldedSyncGenerationTests`, `StateManagementTests`.
- **App unit tests**: QR parser, recipient validation, resumable filter, key-disable gate,
  pasteboard candidate — direct ports of `QRPayloadParserTests`, `CreateIdentityResumableTests`,
  `KeyDisableGateTests`, `SendViewModelCoreRecipientsTests`, `WalletDeletionTests`.
- **Compose UI tests** + launch test ≈ `SwiftExampleAppUITests`.
- Live-network tests gated on a `.env` analogue (`local.properties` / env) like the Swift
  `StateTransitionTests`. Skip gracefully when native lib / credentials absent (existing
  `assumeTrue` pattern).

### Phase 10 — Docs, CI, packaging
- README + `AI_REFERENCE.md` analogue; KDoc on public API.
- CI: build native (NDK), assemble AAR, run JVM + instrumented tests on emulator.
- Publish AAR (Maven coordinates), keep `console` working, update root `CLAUDE.md` iOS section
  with a Kotlin counterpart.

---

## 8. Subsystem port map (quick reference)

| Swift (`Sources/SwiftDashSDK/…`) | Kotlin target | Phase |
|---|---|---|
| `SDK.swift`, `SwiftDashSDK.swift`, `DashNetwork.swift`, `ConcurrencyCompat.swift` | `DashSDK.kt`, `Network`, coroutine scopes | 7 / 1 |
| `FFI/*` | `ffi/*` (bindings + callbacks) | 1, 4 |
| `DPP/*`, `Models/*` | `dpp/*`, `models/*` (`@Serializable`) | 2 |
| `Utils/*`, `Helpers/*`, `Address/*`, `Wallet/*`, `Config/*` | `utils/`, `helpers/`, `address/`, `config/` | 2, 7 |
| `Persistence/*` (29 `@Model`), `Services/DataManager`, `Core/Utils/DataContractParser` | `persistence/*` (Room) | 3 |
| `Security/*`, `Core/Wallet/WalletStorage` | `security/*` (Keystore) | 4 |
| `KeyWallet/*` | `keywallet/*` | 5 |
| `PlatformWallet/*` (+ subdirs) | `platformwallet/*` + `state/*` | 6 |
| `Core/Services/SDKLogger` | `core/SdkLogger.kt` | 7 |
| `SwiftExampleApp/*` | `example-app/*` (Compose) | 8 |
| `SwiftTests/*`, `SwiftExampleAppTests/*`, `*UITests` | `src/test`, `src/androidTest`, Compose UI tests | 9 |

---

## 9. Risks & open questions

1. **Native surface — RESOLVED (§6).** Build & bundle the existing `rs-unified-sdk-ffi`
   cdylib (`--features shielded`) instead of just `rs-sdk-ffi`. `key-wallet-ffi` is an
   auto-pulled Cargo git dep (no checkout needed); headers are emitted by cbindgen on every
   build. Remaining unknown: whether all transitive C deps (e.g. openssl-sys) cross-compile
   cleanly for all 4 Android ABIs under the unified crate — validate in Phase 0 by actually
   running the modified `build_android.sh`.
2. **JNA callbacks across NDK** for signer/persister/resolver: confirm lifetime/threading
   (pin callback objects; persister runs off-main; signer materializes key briefly). Fallback:
   JNI shim for the callback-heavy paths if JNA proves fragile.
3. **SwiftData → Room fidelity:** 29 entities with composite uniqueness, cascade rules, and
   network-scoped queries. Migrations must stay additive; verify `walletGroupId`/`networkRaw`
   index parity for per-network scans.
4. **Shielded (Orchard) pool:** ensure the Android native build includes Orchard support and
   bech32m address parsing; no Kotlin-side crypto.
5. **Biometric/Keystore semantics** differ from iOS Keychain (no exact
   `WhenUnlockedThisDeviceOnly` match) — pick `setUserAuthenticationRequired` + StrongBox where
   available; document the security posture difference.
6. **JSON strategy:** migrate existing regex parsing to `kotlinx.serialization` early (Phase 2)
   to avoid carrying it into new code.
7. **Effort:** this is a large port (~42k LOC SDK + 138-file app + ~290 tests). Treat phases as
   milestones, not a single PR.

---

## 10. Suggested execution approach

- Use the **`kotlin-master-engineer`** agent for implementation phases and
  **`kotlin-quality-engineer`** for review gates (no force-unwraps/unsafe casts, file/test
  placement, idiomatic coroutines/Flow).
- One PR per phase (or per subsystem within a phase), each green-building with its tests.
- Land Phase 0 + 1 first and prove a single non-trivial `platform_wallet_*` call end-to-end
  (create manager → callback → Room write) before scaling out — it validates the whole
  native+callback+persistence spine.
- Keep the read-path `console` and existing services working throughout as a regression anchor.

---

### Appendix: source inventory (survey snapshot)
- `Sources/SwiftDashSDK`: 112 Swift files, ~41,803 LOC. Largest dirs: `Persistence/Models` (28),
  `PlatformWallet` (20), `KeyWallet` (20), `Utils` (7), `FFI` (5), `DPP` (5).
- `SwiftExampleApp`: 138 Swift files (≈109 in app target) — 5 tabs, SwiftUI + SwiftData.
- Tests: `SwiftDashSDKTests` 12 files / ~206 fns; `SwiftExampleAppTests` 10 files / ~79 fns;
  UI tests 2 files.
- Existing `kotlin-sdk`: 51 files; read-only SDK over JNA; bundles `librs_sdk_ffi.so` (4 ABIs).
