package org.dash.sdk

import org.dash.sdk.ffi.DashSDKConfigNative
import org.dash.sdk.ffi.DashSDKNetwork
import org.dash.sdk.ffi.DashSdkFfi
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Integration-style test that exercises the native library directly via JNA.
 *
 * Prerequisites:
 *   Run `./build_local.sh` (Mac/Linux) or `build_local.bat` (Windows) to build
 *   the Rust library for the host platform. Gradle will automatically point JNA
 *   at the output directory (target/release/) via the `dash.sdk.lib.dir`
 *   system property configured in build.gradle.kts.
 *
 * Tests are skipped automatically when the native library is not present.
 */
class NativeSdkTest {

    private val nativeLibAvailable: Boolean
        get() = System.getProperty("dash.sdk.lib.dir") != null

    @Before
    fun skipIfNoNativeLib() {
        assumeTrue(
            "Native library not found — run build_local.sh first",
            nativeLibAvailable
        )
    }

    /**
     * Calls dash_sdk_create with a mock SDK config (no DAPI addresses = no network needed),
     * verifies success, then calls dash_sdk_destroy on the returned handle.
     */
    @Test
    fun testCreateAndDestroy() {
        val ffi = DashSdkFfi.INSTANCE

        val config = DashSDKConfigNative().apply {
            network = DashSDKNetwork.TESTNET
            dapi_addresses = null          // null → mock SDK, no network access
            skip_asset_lock_proof_verification = 1
            request_retry_count = 1
            request_timeout_ms = 5_000L
        }

        val result = ffi.dash_sdk_create(config)

        assertNull("dash_sdk_create returned an unexpected error", result.error)
        assertNotNull("dash_sdk_create returned a null SDK handle", result.data)

        ffi.dash_sdk_destroy(result.data!!)
    }

    /**
     * Verifies that creating two independent SDK handles and destroying both
     * does not crash (double-init / double-destroy safety).
     */
    @Test
    fun testCreateTwoHandles() {
        val ffi = DashSdkFfi.INSTANCE

        val config = DashSDKConfigNative().apply {
            network = DashSDKNetwork.TESTNET
            dapi_addresses = null
            skip_asset_lock_proof_verification = 1
            request_retry_count = 1
            request_timeout_ms = 5_000L
        }

        val result1 = ffi.dash_sdk_create(config)
        val result2 = ffi.dash_sdk_create(config)

        assertNull("First create returned an error", result1.error)
        assertNull("Second create returned an error", result2.error)
        assertNotNull("First handle is null", result1.data)
        assertNotNull("Second handle is null", result2.data)

        ffi.dash_sdk_destroy(result1.data!!)
        ffi.dash_sdk_destroy(result2.data!!)
    }
}
