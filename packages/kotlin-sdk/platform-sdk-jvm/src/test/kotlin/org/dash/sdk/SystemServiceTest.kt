package org.dash.sdk

import kotlinx.coroutines.runBlocking
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.models.Network
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the read-path system bindings added in Phase 1a.
 *
 * Only [version] is asserted against behavior — it's a process-local call needing no
 * network. The JSON status/system queries require a live DAPI connection (the mock SDK,
 * `dapiAddresses = null`, has no platform to query), so they are not asserted here; they
 * follow the same proven `DashSDKResult` → `unwrapString` path as the DPNS/document services.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class SystemServiceTest {

    private val nativeLibAvailable: Boolean
        get() = System.getProperty("dash.sdk.lib.dir") != null

    @Before
    fun skipIfNoNativeLib() {
        assumeTrue(
            "Native library not found — run build_platform_local.sh first",
            nativeLibAvailable
        )
    }

    /** `dash_sdk_version` returns a non-blank static string with no SDK handle or network. */
    @Test
    fun versionReturnsNonBlank() {
        val version = DashSdkFfi.INSTANCE.dash_sdk_version()
        assertNotNull("dash_sdk_version returned null", version)
        assertTrue("dash_sdk_version returned blank", version!!.isNotBlank())
    }

    /** The facade exposes the same version through the SystemService wrapper. */
    @Test
    fun systemServiceVersionMatchesFfi() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val viaService = sdk.system.version()
            val viaFfi = DashSdkFfi.INSTANCE.dash_sdk_version().orEmpty()
            assertTrue("SystemService.version() was blank", viaService.isNotBlank())
            assertTrue("version mismatch between facade and FFI", viaService == viaFfi)
        }
    }

    /** `dash_sdk_init` is idempotent and must not crash. */
    @Test
    fun initDoesNotCrash() {
        DashSdkFfi.INSTANCE.dash_sdk_init()
    }
}
