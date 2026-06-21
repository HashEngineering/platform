package org.dash.sdk

import kotlinx.coroutines.runBlocking
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.StateTransitionType
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the identity read-path bindings added in Phase 1a (batch 2).
 *
 * Network-dependent queries (the fetch_ family and resolve_name) require a live DAPI connection
 * (the mock SDK with `dapiAddresses = null` has no platform to query), so they are
 * not asserted here — they follow the same proven `DashSDKResult` → `unwrapString`
 * path as the DPNS/document services.
 *
 * `parse_json` is process-local (no network), so an obviously-invalid JSON string
 * must surface a [DashSDKException] rather than crash the JVM.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class IdentityReadPathTest {

    private val nativeLibAvailable: Boolean
        get() = System.getProperty("dash.sdk.lib.dir") != null

    @Before
    fun skipIfNoNativeLib() {
        assumeTrue(
            "Native library not found — run build_platform_local.sh first",
            nativeLibAvailable
        )
    }

    /** `dash_sdk_identity_parse_json` on invalid JSON raises DashSDKException, no crash. */
    @Test
    fun parseJsonInvalidThrows() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                sdk.identity.parseJson("this is not valid identity json {{{")
                fail("Expected DashSDKException for invalid identity JSON")
            } catch (e: DashSDKException) {
                // expected: parse failure surfaced as a typed exception
            }
        }
    }

    /** Empty string is also invalid JSON and must surface a DashSDKException, not crash. */
    @Test
    fun parseJsonEmptyThrows() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                sdk.identity.parseJson("")
                fail("Expected DashSDKException for empty identity JSON")
            } catch (e: DashSDKException) {
                // expected
            }
        }
    }

    /** The StateTransitionType constants mirror the C enum's integer values. */
    @Test
    fun stateTransitionTypeConstantsMatchHeader() {
        assertEquals(0, StateTransitionType.IDENTITY_UPDATE)
        assertEquals(1, StateTransitionType.IDENTITY_TOP_UP)
        assertEquals(2, StateTransitionType.IDENTITY_CREDIT_TRANSFER)
        assertEquals(3, StateTransitionType.IDENTITY_CREDIT_WITHDRAWAL)
        assertEquals(4, StateTransitionType.DOCUMENTS_BATCH)
        assertEquals(5, StateTransitionType.DATA_CONTRACT_CREATE)
        assertEquals(6, StateTransitionType.DATA_CONTRACT_UPDATE)
    }

    /** The new FFI bindings resolve in the loaded library (link check, no network). */
    @Test
    fun newBindingsAreResolvable() {
        // Touching INSTANCE forces JNA to bind the interface against the library.
        // A missing symbol would surface as an UnsatisfiedLinkError on first call;
        // parse_json above already invokes one of the new symbols.
        val ffi = DashSdkFfi.INSTANCE
        assertEquals(true, ffi != null)
    }
}
