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
            val jnaPath = System.getProperty("jna.library.path")
            when {
                libDir != null -> {
                    // Explicit lib dir (e.g. desktop unit tests via testOptions)
                    System.setProperty("jna.library.path",
                        if (jnaPath.isNullOrEmpty()) libDir else "$libDir:$jnaPath")
                }
                jnaPath != null -> {
                    // jna.library.path set externally (e.g. console -Djna.library.path=...) — no-op
                }
                else -> {
                    // Android: library is packaged in jniLibs and extracted at install time
                    System.loadLibrary(NativeLibrary.name)
                }
            }
            loaded = true
        }
    }
}
