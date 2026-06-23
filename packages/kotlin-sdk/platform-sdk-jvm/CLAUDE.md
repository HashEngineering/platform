# platform-sdk-jvm — read-path SDK (pure Kotlin/JVM)

This module holds **all the read-path SDK code** — the platform flavor (`rs-sdk-ffi`).
It is plain Kotlin/JVM with **no Android dependency** — only the JVM standard library,
kotlinx-coroutines, and JNA.

If you are adding read-path SDK logic — FFI bindings, models, services, the `DashSDK`
facade, identity/document/DPNS wrappers — it goes **here**. The unified flavor builds
on top of it (`unified-sdk-jvm` does `api(project(":platform-sdk-jvm"))`).

## Where this sits

```
platform-sdk-jvm/      ← you are here (read-path SDK code; the platform flavor)
platform-sdk-android/  api(:platform-sdk-jvm) + librs_sdk_ffi.so
unified-sdk-jvm/       api(:platform-sdk-jvm) + unified-only wallet/shielded bindings; pins rs_unified_sdk_ffi
unified-sdk-android/   api(:unified-sdk-jvm) + librs_unified_sdk_ffi.so
```

The unified library is a **superset** of the read-path symbols, so everything here
works unchanged against it. The rule that decides placement is **which library exports
the symbol**, not whether the operation reads or writes: a binding belongs here iff its
symbol is exported by the read-path `rs_sdk_ffi` library (this module must stay loadable
against it). On that basis some **write/signing** code lives here — the external signer
(`org.dash.sdk.signing`) and identity registration with a caller-supplied asset-lock proof
(`IdentityService.register*`) use only `rs_sdk_ffi` symbols. What stays in
`:unified-sdk-jvm` is anything needing the unified-only symbols: **wallet** funding
(turning UTXOs into an asset lock), **shielded** ops, and the wallet-funded
register/top-up one-shots. When adding a new write binding, `nm`-check the read-path
`librs_sdk_ffi` first (see IDENTITY_REGISTRATION_DESIGN.md §0.1).

## Layout

```
platform-sdk-jvm/src/main/kotlin/org/dash/sdk/
├── DashSDK.kt          facade (AutoCloseable; create/close, exposes services)
├── ffi/                JNA bindings + native-lib loading
│   ├── DashSdkFfi.kt   JNA Library interface + repr(C) Structures
│   ├── NativeLoader.kt loads the .so/.dylib (host path vs Android System.loadLibrary)
│   ├── NativeLibrary.kt resolves which library to load (see below)
│   └── ResultUnwrapper.kt error/result unwrapping → DashSDKException
├── models/             data classes + enums (Identity, Document, DataContract, Network, …)
└── services/           suspend services (Identity, DataContract, Document, DPNS)
platform-sdk-jvm/src/test/  host JVM unit tests (run on the desktop JVM, not an emulator)
```

## Conventions

- **Service methods are `suspend`** and dispatch on `Dispatchers.IO`. FFI calls are
  blocking; never run them on the caller's thread.
- **Handles are freed deterministically** via `AutoCloseable`/`close()`, not finalizers.
- **Errors:** unwrap every FFI result through `ResultUnwrapper`; surface failures as
  `DashSDKException`. Free native error/string pointers after reading them.
- **Native library selection** is centralized in `NativeLibrary`. Resolution order:
  `dash.sdk.native.lib` system property → `dash-sdk-native.properties` classpath
  resource (shipped only by `:unified-sdk-jvm`) → `rs_sdk_ffi` fallback. This module
  ships **no** resource — it relies on the fallback. Do not hard-code library names in
  new `Native.load` / `System.loadLibrary` calls — go through `NativeLibrary.name`.

## Architectural rule (inherited from the Swift SDK)

This SDK **persists, loads, and bridges** — it does not contain business logic. No
gap-limit walks, no derivation-path building, no multi-step orchestration. Every
binding is a 1:1 wrapper over a Rust FFI function (marshal in → call → marshal out).
If something needs deciding (which index/path/key/order), it belongs in the Rust
crates (`rs-sdk` / `key-wallet` / `platform-wallet`), reached through a single FFI
call. See `../../swift-sdk/CLAUDE.md` for the canonical statement of this rule.

## Tests

Host JVM unit tests live in `src/test`. They need a host native library — build the
flavor you want first:

```bash
../build_platform_local.sh                          # rs-sdk-ffi
../gradlew :platform-sdk-jvm:test

../build_unified_local.sh                           # rs-unified-sdk-ffi
../gradlew :platform-sdk-jvm:test -PdashNativeLib=rs_unified_sdk_ffi
```

The `tasks.withType<Test>` block in `build.gradle.kts` resolves
`lib<dashNativeLib>.{dylib,so}` from `<repo-root>/target/release` and skips native tests
(`assumeTrue`) if it is absent.

## Native library

Built by `../build_platform_local.sh` / `../build_unified_local.sh` (host, for tests)
into `<repo-root>/target/release/`. JNA finds it via `jna.library.path` /
`dash.sdk.lib.dir`. The generated cbindgen headers (the source of truth for new
bindings) are collected under `../native/include/` by the Android build scripts.
