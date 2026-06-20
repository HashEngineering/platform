# sdk — Android library wrapper

This module is the **Android delivery layer**. It is a `com.android.library`
(produces an `.aar`) that wraps the pure-JVM core and adds Android-specific packaging.

It deliberately contains **almost no SDK logic**. The SDK code lives in `:sdk-jvm`,
which this module re-exports:

```kotlin
api(project(":sdk-jvm"))
```

`api` (not `implementation`) so that `sdk-jvm`'s public types are visible to Android
consumers of the `.aar`.

If you are adding cross-platform SDK logic, put it in `:sdk-jvm`. Only put code here
when it **requires the Android SDK** (see below).

## What belongs in this module

- **Native packaging:** the `librs_sdk_ffi.so` / `librs_unified_sdk_ffi.so` files
  under `src/main/jniLibs/<abi>/`, loaded at runtime via `System.loadLibrary`
  (the base name comes from `org.dash.sdk.ffi.NativeLibrary`).
- **`abiFilters`:** `arm64-v8a` and `x86_64` only — **64-bit ABIs**. The 32-bit ABIs
  (`armeabi-v7a`, `x86`) are intentionally unsupported because the unified FFI structs
  carry hard-coded 64-bit size/alignment guards (matching the iOS aarch64-only
  framework) and do not compile for 32-bit. Keep this filter in sync with
  `../build_android.sh`.
- `AndroidManifest.xml` (INTERNET permission), `proguard-rules.pro`,
  `consumer-rules.pro`.
- Android-flavored dependencies: `kotlinx-coroutines-android` and the JNA **`@aar`**
  variant (which ships `libjnidispatch.so` for the Android runtime; the plain JAR is
  used only for host unit tests).
- **Future Android-only implementations** (per the porting plan): Room persistence
  (`@Entity`/DAO/database), Android Keystore (signer, biometric, key storage), and
  `StateFlow` holders mirroring the Swift `PlatformWalletManager` observables.

## Tests

`src/test` holds **host JVM unit tests** (they run on the desktop JVM, not an
emulator). They need the host native library — build it first:

```bash
../build_local.sh                                   # rs-sdk-ffi
../gradlew :sdk:testDebugUnitTest

../build_local.sh --unified                         # rs-unified-sdk-ffi
../gradlew :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi
```

The `testOptions` block resolves `lib<dashNativeLib>.{dylib,so}` from
`<repo-root>/target/release` and skips native tests if it is absent.
Instrumented/Android tests (when added) go in `src/androidTest`.

## Gotchas

- Both native libraries (`librs_sdk_ffi.so`, `librs_unified_sdk_ffi.so`) are
  git-ignored; regenerate with `../build_android.sh`. Don't commit them.
- Don't duplicate `sdk-jvm` code here. If a type "isn't visible," fix it in `sdk-jvm`
  and rely on the `api(project(":sdk-jvm"))` dependency.

See `../README.md` for the overall two-module rationale and `../sdk-jvm/CLAUDE.md`
for the core conventions.
