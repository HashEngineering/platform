package org.dash.sdk.ffi

/**
 * Loads the native rs-sdk-ffi shared library.
 * Call [load] once before using any FFI functions.
 */
object NativeLoader {
    private var loaded = false

    fun load() {
        if (!loaded) {
            val libDir = System.getProperty("dash.sdk.lib.dir")
            if (libDir != null) {
                // Desktop JVM (tests): tell JNA where to find librs_sdk_ffi
                val existing = System.getProperty("jna.library.path")
                System.setProperty(
                    "jna.library.path",
                    if (existing.isNullOrEmpty()) libDir else "$libDir:$existing"
                )
            } else {
                // Android: library is packaged in jniLibs and extracted at install time
                System.loadLibrary("rs_sdk_ffi")
            }
            loaded = true
        }
    }
}
