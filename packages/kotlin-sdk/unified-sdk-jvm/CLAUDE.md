# unified-sdk-jvm — unified flavor (pure JVM)

Pure-Kotlin/JVM module that selects the **unified** flavor (full SDK + key-wallet +
platform-wallet + shielded, `rs-unified-sdk-ffi`). The unified native library is a
**superset** of the read-path symbols, so this module builds on the platform flavor:

```kotlin
api(project(":platform-sdk-jvm"))   // re-exports all the read-path code
```

Its flavor marker is `src/main/resources/dash-sdk-native.properties`, which pins
`dash.sdk.native.lib=rs_unified_sdk_ffi`. `platform-sdk-jvm` intentionally ships **no**
such resource (it relies on the `rs_sdk_ffi` fallback), so even though this module
depends on it, the unified classpath has exactly one `dash-sdk-native.properties` — no
ambiguity. `org.dash.sdk.ffi.NativeLibrary` reads it from the classpath, so depending on
this module loads the unified library with no system property.

**This is the home for unified-only bindings.** The net-new wallet/shielded surface from
`SWIFT_TO_KOTLIN_PORTING_PLAN.md` (KeyWallet, PlatformWallet, shielded/Orchard, signing,
broadcast) goes here — it relies on symbols that exist only in `rs-unified-sdk-ffi`.
Read-path code shared with the platform flavor stays in `:platform-sdk-jvm`.

See `../README.md` and `../platform-sdk-jvm/CLAUDE.md`.
