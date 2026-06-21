# platform-sdk-android — Android delivery layer (platform flavor)

This module is the **Android delivery layer for the platform flavor** (read-path,
`rs-sdk-ffi`). It is a `com.android.library` (produces an `.aar`) that wraps the
pure-JVM flavor module and adds Android-specific packaging.

It deliberately contains **almost no SDK logic**. The read-path SDK code lives in
`:platform-sdk-jvm`, which this module re-exports:

```kotlin
api(project(":platform-sdk-jvm"))
```

`api` (not `implementation`) so the public types are visible to Android consumers of
the `.aar`.

If you are adding cross-platform read-path SDK logic, put it in `:platform-sdk-jvm`.
Only put code here when it **requires the Android SDK** (see below). The unified
flavor's counterpart is `:unified-sdk-android`.

## What belongs in this module

- **Native packaging:** `librs_sdk_ffi.so` under `src/main/jniLibs/<abi>/`, loaded at
  runtime via `System.loadLibrary` (the base name comes from
  `org.dash.sdk.ffi.NativeLibrary`, resolved to `rs_sdk_ffi` by the
  `dash-sdk-native.properties` resource in `:platform-sdk-jvm`). Only the read-path
  `.so` ships here — the unified `.so` lives in `:unified-sdk-android`.
- **`namespace = "org.dash.sdk.platform"`** — distinct from `:unified-sdk-android`
  (`org.dash.sdk.unified`) so the two modules' generated `R` classes don't collide.
- **`abiFilters`:** `arm64-v8a` and `x86_64` only — **64-bit ABIs**. The 32-bit ABIs
  (`armeabi-v7a`, `x86`) are intentionally unsupported (the unified FFI structs carry
  hard-coded 64-bit size/alignment guards). Keep this filter in sync with
  `../build_native.sh`.
- `AndroidManifest.xml` (INTERNET permission), `proguard-rules.pro`, `consumer-rules.pro`.
- Android-flavored dependencies: `kotlinx-coroutines-android` and the JNA **`@aar`**
  variant (which ships `libjnidispatch.so` for the Android runtime; the plain JAR is
  used only for host unit tests, which live in `:platform-sdk-jvm`).
- **Future Android-only implementations** (per the porting plan): Room persistence,
  Android Keystore (signer, biometric, key storage), and `StateFlow` holders mirroring
  the Swift `PlatformWalletManager` observables. Note most of those are wallet/shielded
  features and so belong on the **unified** side.

## Tests

There are no host JVM unit tests here — they live in `:platform-sdk-jvm/src/test`. Build
the host library and run them against the JVM module:

```bash
../build_platform_local.sh
../gradlew :platform-sdk-jvm:test
```

Instrumented/Android tests (when added) go in `src/androidTest`.

## Gotchas

- `librs_sdk_ffi.so` is git-ignored; regenerate with `../build_platform_android.sh`.
  Don't commit it.
- Don't duplicate `:platform-sdk-jvm` code here. If a type "isn't visible," fix it in
  `:platform-sdk-jvm` and rely on the `api(project(":platform-sdk-jvm"))` dependency.

See `../README.md` for the overall flavor/module rationale and
`../platform-sdk-jvm/CLAUDE.md` for the read-path SDK conventions.
