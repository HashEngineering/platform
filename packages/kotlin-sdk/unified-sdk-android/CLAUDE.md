# unified-sdk-android — Android delivery layer (unified flavor)

The unified-flavor counterpart of `:platform-sdk-android`. Same structure, but it
packages the **unified** native library and wraps the unified JVM flavor:

```kotlin
api(project(":unified-sdk-jvm"))   // which does api(project(":platform-sdk-jvm"))
```

The unified flavor is the full SDK + key-wallet + platform-wallet + shielded
(`rs-unified-sdk-ffi`), a superset of the read-path symbols.

## What differs from `:platform-sdk-android`

- **Native packaging:** `librs_unified_sdk_ffi.so` under `src/main/jniLibs/<abi>/`
  (resolved by the `dash-sdk-native.properties` resource in `:unified-sdk-jvm`). Only
  the unified `.so` ships here.
- **`namespace = "org.dash.sdk.unified"`** — distinct from `:platform-sdk-android`
  (`org.dash.sdk.platform`) so generated `R` classes don't collide.
- Regenerate the `.so` with `../build_unified_android.sh` (git-ignored; don't commit).

Everything else (abiFilters, manifest, ProGuard, JNA `@aar`, the "almost no logic here"
rule) matches `:platform-sdk-android` — see its `CLAUDE.md`. Shared read-path code goes
in `:platform-sdk-jvm`; **unified-only** wallet/shielded bindings go in `:unified-sdk-jvm`.
