package org.dash.sdk.ffi

/**
 * Resolves which native FFI library the SDK loads at runtime.
 *
 * Two libraries are produced by `build_android.sh` / `build_local.sh`:
 *  - `rs_sdk_ffi`         — the read-path SDK (default; preserves existing behavior).
 *  - `rs_unified_sdk_ffi` — the unified library (full SDK + key-wallet + platform-wallet
 *                           + shielded), a superset of the read-path symbols.
 *
 * The default stays `rs_sdk_ffi` so nothing changes unless explicitly opted in. Override via
 * the `dash.sdk.native.lib` system property, e.g. `-Ddash.sdk.native.lib=rs_unified_sdk_ffi`.
 *
 * The value is the base name (no `lib` prefix, no extension) — the form
 * [System.loadLibrary] and JNA's `Native.load` both expect.
 */
object NativeLibrary {
    const val READ_PATH = "rs_sdk_ffi"
    const val UNIFIED = "rs_unified_sdk_ffi"

    const val PROPERTY = "dash.sdk.native.lib"

    /** Base name of the native library to load (default [READ_PATH]). */
    val name: String
        get() = System.getProperty(PROPERTY)?.takeIf { it.isNotBlank() } ?: READ_PATH
}
