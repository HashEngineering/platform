# Kotlin Dash SDK

Kotlin/JVM and Android bindings for the Dash Platform SDK, built on the Rust FFI
layer (`rs-sdk-ffi` / the unified `rs-unified-sdk-ffi`) via [JNA](https://github.com/java-native-access/jna).

This is the Kotlin counterpart to the Swift SDK in `packages/swift-sdk`. The
long-term plan to bring it to feature parity lives in
[`SWIFT_TO_KOTLIN_PORTING_PLAN.md`](SWIFT_TO_KOTLIN_PORTING_PLAN.md).

## Module layout

This is a Gradle multi-module project (`rootProject.name = "KotlinDashSDK"`). The
SDK is split into a **pure-JVM core** and a **thin Android wrapper** — one codebase,
two delivery targets.

```
kotlin-sdk/
├── sdk-jvm/   ← all SDK code (pure Kotlin/JVM): FFI bindings, models, services, DashSDK
├── sdk/       ← Android library (.aar): wraps :sdk-jvm + bundles native .so files
└── console/   ← desktop CLI demo (DPNS search), depends on :sdk-jvm only
```

| Module | Plugin | Runs on | Role |
|---|---|---|---|
| **`sdk-jvm`** | `org.jetbrains.kotlin.jvm` | any JVM (desktop, server, CLI, unit tests) | The actual SDK. No Android dependency. |
| **`sdk`** | `com.android.library` + `kotlin-android` | Android only | Packaging layer for Android. `api(project(":sdk-jvm"))` + native `.so`s. |
| **`console`** | `kotlin.jvm` + `application` | desktop JVM | Example CLI. `implementation(project(":sdk-jvm"))`. |

### Why two SDK modules (`sdk-jvm` vs `sdk`)?

The SDK logic is plain Kotlin and needs only the JVM + JNA — it has **no Android
dependency**. Keeping it in `sdk-jvm` (rather than directly in the Android module)
buys three things:

1. **Desktop/JVM use without the Android toolchain.** The `console` CLI and any
   server-side consumer depend on `:sdk-jvm` and never pull in AGP or the Android SDK.
2. **Fast host unit tests.** Tests run on the host JVM against a locally-built
   `librs_sdk_ffi.dylib`/`.so` — no emulator. See [Testing](#testing).
3. **Clean Android packaging.** `sdk` re-exports `sdk-jvm` (`api(...)`, so its types
   are visible to consumers) and adds only the Android-specific pieces:
   - `AndroidManifest.xml` (INTERNET permission), ProGuard/consumer rules
   - `abiFilters` (`arm64-v8a`, `x86_64`) and `jniLibs/` packaging of the native `.so`
   - `kotlinx-coroutines-android` and the JNA **`@aar`** variant (ships
     `libjnidispatch.so` for the Android runtime)

`sdk` deliberately contains almost no Kotlin source — it is the Android delivery
wrapper around `sdk-jvm`. As the port grows (see the plan), `sdk` is where the
Android-only implementations land: Room persistence, Android Keystore, and the
`StateFlow` holders.

### How the native library is loaded

Two native libraries are produced by the build scripts:

- **`rs_sdk_ffi`** — the read-path SDK (**default**; preserves existing behavior).
- **`rs_unified_sdk_ffi`** — the unified library (full SDK + key-wallet +
  platform-wallet + shielded), a superset of the read-path symbols.

Selection is centralized in `org.dash.sdk.ffi.NativeLibrary` (default `rs_sdk_ffi`),
overridable via the `dash.sdk.native.lib` system property or the `dashNativeLib`
Gradle property. Loading differs per target:

- **`sdk-jvm` (desktop/CLI/tests):** JNA loads from a host directory
  (`jna.library.path` / `dash.sdk.lib.dir`), pointing at `<repo-root>/target/release`.
- **`sdk` (Android):** the `.so` is packaged in `jniLibs/<abi>/` and loaded with
  `System.loadLibrary`.

> The Cargo workspace shares a single `target/` at the **repo root**
> (`…/platform/target/release`), *not* under `packages/kotlin-sdk/`. That is where
> `build_local.sh` writes the host library and where Gradle's test config looks.

## Building the native library

Two helper scripts wrap `cargo`. Both default to `rs-sdk-ffi`; pass `--unified` for
the full library.

```bash
# Host library (for JVM unit tests) → <repo-root>/target/release/lib<name>.{dylib,so}
./build_local.sh                 # librs_sdk_ffi.dylib / .so
./build_local.sh --unified       # librs_unified_sdk_ffi.dylib / .so
# Windows: build_local.bat [--unified]

# Android .so files → sdk/src/main/jniLibs/<abi>/  (64-bit ABIs only)
./build_android.sh                # arm64-v8a + x86_64, rs-sdk-ffi
./build_android.sh all --unified  # arm64-v8a + x86_64, unified
./build_android.sh arm64 --unified
```

**64-bit only.** Supported ABIs are `arm64-v8a` (devices) and `x86_64` (emulator).
The 32-bit ABIs (`armeabi-v7a`, `x86`) are intentionally unsupported: the unified
FFI structs carry hard-coded 64-bit size/alignment guards (matching the iOS
aarch64-only framework), so 32-bit targets do not compile.

Requirements for the Android build: Android NDK (`ANDROID_NDK_HOME` or a standard
SDK location) and the Rust targets `aarch64-linux-android`, `x86_64-linux-android`
(auto-installed by the script).

## Building the Kotlin artifacts

```bash
./gradlew :sdk:assembleRelease    # Android .aar
./gradlew :console:run            # run the CLI demo
```

## Testing

JVM unit tests live in `sdk/src/test` and run on the host. Build the host library
first, then run Gradle; the build picks up the library from `<repo-root>/target/release`:

```bash
./build_local.sh
./gradlew :sdk:testDebugUnitTest

# against the unified library:
./build_local.sh --unified
./gradlew :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi
```

Native-dependent tests auto-skip (`assumeTrue`) when the host library is absent.

## Toolchain

Kotlin 2.0.21 · Coroutines 1.9.0 · JNA 5.14.0 · Gradle 8.9 · AGP 8.5.2 · JVM 17 ·
Android minSdk 24 / compileSdk 35.

## Notes

- `bin/` directories are IDE (Eclipse/VS Code) build output — they mirror `src/` and
  are git-ignored. The canonical sources are under each module's `src/main/kotlin`.
- Native `.so`/`.dylib` artifacts and `native/include/` (collected cbindgen headers)
  are git-ignored; regenerate them with the build scripts.
