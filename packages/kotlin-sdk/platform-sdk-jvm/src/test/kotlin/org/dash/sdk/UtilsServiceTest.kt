package org.dash.sdk

import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the process-local utility bindings (base58/hex conversion, base58 validation,
 * platform-address encoding, GroveDB proof formatting). All of these run offline — no DAPI
 * connection — so behavior is asserted directly.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class UtilsServiceTest {

    // Canonical DPNS data-contract ID (base58, 32 bytes) — a stable, valid base58 value.
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

    /** base58 → hex → base58 round-trips back to the original value (both directions marshal). */
    @Test
    fun base58HexRoundTrips() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val hex = sdk.utils.base58ToHex(dpnsContractId)
            assertTrue("base58ToHex returned blank", hex.isNotBlank())
            val back = sdk.utils.hexToBase58(hex)
            assertEquals("base58 → hex → base58 did not round-trip", dpnsContractId, back)
        }
    }

    /** isValidBase58 returns true for a valid base58 string and false for invalid input. */
    @Test
    fun isValidBase58DiscriminatesInput() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            assertTrue("valid base58 reported invalid", sdk.utils.isValidBase58(dpnsContractId))
            // '0', 'O', 'I', 'l' and spaces are not in the base58 alphabet.
            assertFalse("invalid base58 reported valid", sdk.utils.isValidBase58("0OIl not base58!"))
        }
    }

    /** An invalid base58 string surfaces a typed [DashSDKException] rather than crashing. */
    @Test
    fun base58ToHexInvalidThrows() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                sdk.utils.base58ToHex("0OIl not base58!")
                fail("Expected DashSDKException for invalid base58")
            } catch (e: DashSDKException) {
                // expected
            }
        }
    }

    /**
     * Encoding a well-formed 25-byte P2PKH scriptPubKey yields a non-blank platform address —
     * confirms the ByteArray → Memory + FFINetwork marshalling path.
     */
    @Test
    fun encodePlatformAddressReturnsNonBlank() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            // OP_DUP OP_HASH160 <20-byte push> OP_EQUALVERIFY OP_CHECKSIG
            val script = ByteArray(25)
            script[0] = 0x76.toByte()
            script[1] = 0xa9.toByte()
            script[2] = 0x14.toByte()
            // bytes 3..22 are the 20-byte hash (left as zeroes — valid for encoding)
            script[23] = 0x88.toByte()
            script[24] = 0xac.toByte()
            val address = sdk.utils.encodePlatformAddress(script, Network.TESTNET)
            assertTrue("encodePlatformAddress returned blank", address.isNotBlank())
        }
    }

    /** An empty scriptPubKey is rejected before crossing the FFI boundary. */
    @Test
    fun encodePlatformAddressRejectsEmpty() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                sdk.utils.encodePlatformAddress(ByteArray(0), Network.TESTNET)
                fail("Expected IllegalArgumentException for empty scriptPubKey")
            } catch (e: IllegalArgumentException) {
                // expected
            }
        }
    }
}
