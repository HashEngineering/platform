package org.dash.sdk.ffi

import java.util.Properties

/**
 * Resolves which native FFI library the SDK loads at runtime.
 *
 * Two libraries are produced by the build scripts:
 *  - `rs_sdk_ffi`         — the read-path SDK (the **platform** flavor).
 *  - `rs_unified_sdk_ffi` — the unified library (full SDK + key-wallet + platform-wallet
 *                           + shielded), a superset of the read-path symbols (the **unified** flavor).
 *
 * The library is chosen by **which flavor module is on the classpath**:
 *  - `platform-sdk-jvm` (this module) ships **no** resource and relies on the [READ_PATH]
 *    fallback below, so a platform consumer loads `rs_sdk_ffi`.
 *  - `unified-sdk-jvm` (which depends on `platform-sdk-jvm`) ships a
 *    `dash-sdk-native.properties` resource pinning `rs_unified_sdk_ffi`.
 *
 * Only the unified module ships the resource, so even though unified depends on platform
 * the classpath has exactly one — no ambiguity from the dependency edge. Resolution order:
 *
 *  1. System property `dash.sdk.native.lib` (explicit override, e.g. for tests).
 *  2. Classpath resource `dash-sdk-native.properties` (key `dash.sdk.native.lib`) — shipped by unified.
 *  3. [READ_PATH] constant fallback (the platform flavor's default).
 *
 * The value is the base name (no `lib` prefix, no extension) — the form
 * [System.loadLibrary] and JNA's `Native.load` both expect.
 */
object NativeLibrary {
    const val READ_PATH = "rs_sdk_ffi"
    const val UNIFIED = "rs_unified_sdk_ffi"

    const val PROPERTY = "dash.sdk.native.lib"

    /** Name of the classpath resource a flavor module ships to pin the native library. */
    const val RESOURCE = "dash-sdk-native.properties"

    /** Flavor default read from the classpath resource, or null if no flavor module is present. */
    private val resourceDefault: String? by lazy {
        NativeLibrary::class.java.classLoader
            ?.getResourceAsStream(RESOURCE)
            ?.use { stream ->
                Properties().apply { load(stream) }.getProperty(PROPERTY)?.takeIf { it.isNotBlank() }
            }
    }

    /** Base name of the native library to load (default [READ_PATH]). */
    val name: String
        get() = System.getProperty(PROPERTY)?.takeIf { it.isNotBlank() }
            ?: resourceDefault
            ?: READ_PATH
}
