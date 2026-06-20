# sdk-jvm — pure Kotlin/JVM SDK core

This module holds **all the actual SDK code**. It is plain Kotlin/JVM with **no
Android dependency** — only the JVM standard library, kotlinx-coroutines, and JNA.

If you are adding SDK logic — FFI bindings, models, services, the `DashSDK` facade,
wallet/identity/document/DPNS wrappers — it goes **here**, not in `:sdk`.

## Why this module exists separately from `:sdk`

The SDK only needs a JVM + JNA to run. Isolating it from Android means:
- The `console` CLI and any server-side consumer use it without the Android toolchain.
- Unit tests run fast on the host JVM (no emulator).
- `:sdk` re-exports it (`api(project(":sdk-jvm"))`) and adds only Android packaging.

See the repo `../README.md` for the full module rationale and `../sdk/CLAUDE.md` for
what belongs on the Android side instead.

## Layout

```
sdk-jvm/src/main/kotlin/org/dash/sdk/
├── DashSDK.kt          facade (AutoCloseable; create/close, exposes services)
├── ffi/                JNA bindings + native-lib loading
│   ├── DashSdkFfi.kt   JNA Library interface + repr(C) Structures
│   ├── NativeLoader.kt loads the .so/.dylib (host path vs Android System.loadLibrary)
│   ├── NativeLibrary.kt resolves which library to load (see below)
│   └── ResultUnwrapper.kt error/result unwrapping → DashSDKException
├── models/             data classes + enums (Identity, Document, DataContract, Network, …)
└── services/           suspend services (Identity, DataContract, Document, DPNS)
```

## Conventions

- **Service methods are `suspend`** and dispatch on `Dispatchers.IO`. FFI calls are
  blocking; never run them on the caller's thread.
- **Handles are freed deterministically** via `AutoCloseable`/`close()`, not finalizers.
- **Errors:** unwrap every FFI result through `ResultUnwrapper`; surface failures as
  `DashSDKException`. Free native error/string pointers after reading them.
- **Native library selection** is centralized in `NativeLibrary` (default
  `rs_sdk_ffi`, opt-in `rs_unified_sdk_ffi` via the `dash.sdk.native.lib` system
  property). Do not hard-code library names in new `Native.load` / `System.loadLibrary`
  calls — go through `NativeLibrary.name`.

## Architectural rule (inherited from the Swift SDK)

This SDK **persists, loads, and bridges** — it does not contain business logic. No
gap-limit walks, no derivation-path building, no multi-step orchestration. Every
binding is a 1:1 wrapper over a Rust FFI function (marshal in → call → marshal out).
If something needs deciding (which index/path/key/order), it belongs in the Rust
crates (`rs-sdk` / `key-wallet` / `platform-wallet`), reached through a single FFI
call. See `../../swift-sdk/CLAUDE.md` for the canonical statement of this rule.

## Native library

Built by `../build_local.sh` (host, for tests) into `<repo-root>/target/release/`.
JNA finds it via `jna.library.path` / `dash.sdk.lib.dir`. The generated cbindgen
headers (the source of truth for new bindings) are collected under
`../native/include/` by `../build_android.sh`.
