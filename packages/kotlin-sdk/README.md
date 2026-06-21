# Kotlin Dash SDK

Kotlin/JVM and Android bindings for the Dash Platform SDK, built on the Rust FFI
layer (`rs-sdk-ffi` / the unified `rs-unified-sdk-ffi`) via [JNA](https://github.com/java-native-access/jna).

This is the Kotlin counterpart to the Swift SDK in `packages/swift-sdk`. The
long-term plan to bring it to feature parity lives in
[`SWIFT_TO_KOTLIN_PORTING_PLAN.md`](SWIFT_TO_KOTLIN_PORTING_PLAN.md).

## Two flavors

The SDK ships in two **compile-time flavors**, chosen by which module you depend on:

- **platform** → `rs-sdk-ffi`, the read-path SDK.
- **unified** → `rs-unified-sdk-ffi`, the full SDK + key-wallet + platform-wallet +
  shielded. The unified native library is a *superset* of the read-path symbols, so the
  **unified flavor builds on the platform flavor** and adds the net-new wallet/shielded
  bindings (per the porting plan).

Because unified is a strict superset, there's no separate "core": the read-path code
lives in `platform-sdk-jvm`, and unified depends on it.

## Module layout

This is a Gradle multi-module project (`rootProject.name = "KotlinDashSDK"`):

```
kotlin-sdk/
├── platform-sdk-jvm/      read-path SDK code (pure Kotlin/JVM): FFI bindings, models,
│                          services, DashSDK, NativeLoader/Library + the host unit tests
├── platform-sdk-android/  platform flavor, Android .aar: api(:platform-sdk-jvm) + librs_sdk_ffi.so
├── unified-sdk-jvm/       unified flavor, pure JVM: api(:platform-sdk-jvm) + wallet/shielded
│                          bindings; pins rs_unified_sdk_ffi
├── unified-sdk-android/   unified flavor, Android .aar: api(:unified-sdk-jvm) + librs_unified_sdk_ffi.so
└── console/               desktop CLI demo (DPNS search), depends on :platform-sdk-jvm
```

| Module | Plugin | Runs on | Role |
|---|---|---|---|
| **`platform-sdk-jvm`** | `org.jetbrains.kotlin.jvm` | any JVM | The read-path SDK. No Android dependency. Holds the host unit tests. |
| **`platform-sdk-android`** | `com.android.library` + `kotlin-android` | Android | Platform flavor packaging: `api(:platform-sdk-jvm)` + `librs_sdk_ffi.so`. |
| **`unified-sdk-jvm`** | `kotlin.jvm` | any JVM | Unified flavor: `api(:platform-sdk-jvm)` + a resource pinning `rs_unified_sdk_ffi`. Home of the net-new wallet/shielded bindings. |
| **`unified-sdk-android`** | `com.android.library` + `kotlin-android` | Android | Unified flavor packaging: `api(:unified-sdk-jvm)` + `librs_unified_sdk_ffi.so`. |
| **`console`** | `kotlin.jvm` + `application` | desktop JVM | Example CLI. `implementation(:platform-sdk-jvm)`. |

### Why this shape?

The read-path SDK logic is plain Kotlin and needs only the JVM + JNA — it has **no
Android dependency**. Keeping it in `platform-sdk-jvm` and having unified depend on it
buys:

1. **No drift, no redundant module.** The read-path bindings exist once. Unified is a
   superset of the read-path symbols, so it `api`-depends on the platform flavor rather
   than duplicating it or routing through an empty "core".
2. **Desktop/JVM use without the Android toolchain.** `console` and server-side
   consumers depend on `:platform-sdk-jvm` and never pull in AGP.
3. **Fast host unit tests.** Tests run on the host JVM against a locally-built
   `lib<flavor>.{dylib,so}` — no emulator. See [Testing](#testing).
4. **Clean Android packaging.** Each `*-sdk-android` module re-exports its JVM flavor
   (`api(...)`) and adds only the Android pieces: `AndroidManifest.xml` (INTERNET),
   ProGuard/consumer rules, `abiFilters` (`arm64-v8a`, `x86_64`), `jniLibs/` packaging
   of its `.so`, `kotlinx-coroutines-android`, and the JNA **`@aar`** variant.

The two Android modules use distinct namespaces (`org.dash.sdk.platform` /
`org.dash.sdk.unified`) so their generated `R` classes don't collide.

### How the native library is chosen

Each flavor maps to one native library:

- **`rs_sdk_ffi`** — the read-path SDK (platform flavor).
- **`rs_unified_sdk_ffi`** — the unified library (unified flavor), a superset of the
  read-path symbols.

Selection is centralized in `org.dash.sdk.ffi.NativeLibrary`, which resolves the name in
this order:

1. System property `dash.sdk.native.lib` (explicit override; used by tests).
2. Classpath resource `dash-sdk-native.properties` — shipped **only** by `:unified-sdk-jvm`.
3. `rs_sdk_ffi` fallback.

So a `:platform-sdk-jvm` consumer falls through to `rs_sdk_ffi`; a `:unified-sdk-jvm`
consumer picks up the resource and loads `rs_unified_sdk_ffi`. Because only unified ships
the resource, the dependency edge (unified → platform) never produces two competing
copies on the classpath. Loading differs per target:

- **JVM (desktop/CLI/tests):** JNA loads from a host directory (`jna.library.path` /
  `dash.sdk.lib.dir`), pointing at `<repo-root>/target/release`.
- **Android:** the `.so` is packaged in `jniLibs/<abi>/` and loaded with `System.loadLibrary`.

> The Cargo workspace shares a single `target/` at the **repo root**
> (`…/platform/target/release`), *not* under `packages/kotlin-sdk/`. That is where the
> host build scripts write and where Gradle's test config looks.

## Building the native library

One engine script (`build_native.sh`) plus thin per-flavor wrappers:

```bash
# Host library (for JVM unit tests) → <repo-root>/target/release/lib<name>.{dylib,so}
./build_platform_local.sh        # librs_sdk_ffi.dylib / .so
./build_unified_local.sh         # librs_unified_sdk_ffi.dylib / .so
# Windows: build_local.bat [platform|unified]

# Android .so files → <flavor>-sdk-android/src/main/jniLibs/<abi>/  (64-bit ABIs only)
./build_platform_android.sh              # arm64-v8a + x86_64, rs-sdk-ffi
./build_unified_android.sh               # arm64-v8a + x86_64, rs-unified-sdk-ffi
./build_unified_android.sh arm64         # single arch

# Or call the engine directly:
./build_native.sh <platform|unified> [--target host|android] [arm64|x86_64|all] [--clean]
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
./gradlew :platform-sdk-android:assembleRelease   # platform .aar
./gradlew :unified-sdk-android:assembleRelease     # unified .aar
./gradlew :console:run                             # run the CLI demo
```

## Testing

Host JVM unit tests live in `platform-sdk-jvm/src/test` and run on the host. Build the
host library for the flavor you want first, then run Gradle:

```bash
./build_platform_local.sh
./gradlew :platform-sdk-jvm:test                   # defaults to rs_sdk_ffi

./build_unified_local.sh
./gradlew :platform-sdk-jvm:test -PdashNativeLib=rs_unified_sdk_ffi
```

The build picks up `lib<dashNativeLib>.{dylib,so}` from `<repo-root>/target/release`.
Native-dependent tests auto-skip (`assumeTrue`) when the host library is absent.

## Toolchain

Kotlin 2.0.21 · Coroutines 1.9.0 · JNA 5.14.0 · Gradle 8.9 · AGP 8.5.2 · JVM 17 ·
Android minSdk 24 / compileSdk 35.

## Notes

- `bin/` directories are IDE (Eclipse/VS Code) build output — they mirror `src/` and
  are git-ignored. The canonical sources are under each module's `src/main/kotlin`.
- Native `.so`/`.dylib` artifacts and `native/include/` (collected cbindgen headers)
  are git-ignored; regenerate them with the build scripts.
