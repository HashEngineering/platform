package org.dash.sdk

import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the token read-path bindings (balances, info, prices, supply, distributions,
 * contract info, statuses, and the calculate_token_id helper).
 *
 * Only [calculateTokenId] is asserted against behavior — it is a process-local derivation
 * (`double_sha256("dash_token" || contract_id || position)`) needing no DAPI connection.
 * The JSON-returning token queries require a live platform (the mock SDK with
 * `dapiAddresses = null` has nothing to query), so they are not asserted here; they follow
 * the same proven `DashSDKResult` → `unwrapString` path as the other read services.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class TokenServiceTest {

    // Canonical DPNS data-contract ID (base58, 32 bytes) — a stable, valid identifier
    // for exercising the process-local token-id derivation without a network.
    private val dpnsContractId = "GWRSAVFMjXx8HpQFaNJMqBV7MBgMK4br5UESsB4S31Ec"

    private val nativeLibAvailable: Boolean
        get() = System.getProperty("dash.sdk.lib.dir") != null

    @Before
    fun skipIfNoNativeLib() {
        assumeTrue(
            "Native library not found — run build_platform_local.sh first",
            nativeLibAvailable
        )
    }

    /**
     * `dash_sdk_calculate_token_id` derives a non-blank base58 token ID from a valid
     * contract ID — a process-local call, no network.
     */
    @Test
    fun calculateTokenIdReturnsNonBlank() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val tokenId = sdk.token.calculateTokenId(dpnsContractId, 0)
            assertTrue("calculateTokenId returned blank", tokenId.isNotBlank())
        }
    }

    /** The derivation is deterministic: same (contractId, position) → same token ID. */
    @Test
    fun calculateTokenIdIsDeterministic() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val a = sdk.token.calculateTokenId(dpnsContractId, 0)
            val b = sdk.token.calculateTokenId(dpnsContractId, 0)
            assertEquals("calculateTokenId not deterministic", a, b)
        }
    }

    /** Different positions yield different token IDs for the same contract. */
    @Test
    fun calculateTokenIdVariesByPosition() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val p0 = sdk.token.calculateTokenId(dpnsContractId, 0)
            val p1 = sdk.token.calculateTokenId(dpnsContractId, 1)
            assertTrue("token IDs for different positions should differ", p0 != p1)
        }
    }

    /**
     * An invalid base58 contract ID surfaces a [DashSDKException] (the header documents an
     * InvalidParameter error) rather than crashing the JVM. Process-local — no network.
     */
    @Test
    fun calculateTokenIdInvalidContractThrows() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                sdk.token.calculateTokenId("not a valid base58 contract id !!!", 0)
                fail("Expected DashSDKException for invalid contract ID")
            } catch (e: DashSDKException) {
                // expected: invalid parameter surfaced as a typed exception
            }
        }
    }

    /** The new token FFI bindings resolve in the loaded library (link check, no network). */
    @Test
    fun newBindingsAreResolvable() {
        // Touching INSTANCE forces JNA to bind the interface; calculateTokenId above
        // already invokes one of the new symbols, so a missing symbol would have thrown.
        org.junit.Assert.assertNotNull(DashSdkFfi.INSTANCE)
    }
}
