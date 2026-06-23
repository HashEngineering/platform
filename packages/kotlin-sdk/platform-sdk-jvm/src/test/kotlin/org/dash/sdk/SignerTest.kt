package org.dash.sdk

import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network
import org.dash.sdk.signing.InMemorySigningKeyStore
import org.dash.sdk.signing.KeystoreSigner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the Phase-A signer infrastructure offline (real native calls, no node):
 * the JNA callback trampolines (via `dash_sdk_signer_can_sign`, which routes through the
 * signer's `can_sign` vtable into [KeystoreSigner]'s callback), the v1 sign primitive
 * (`create_from_private_key` → `sign` → `signature_free`), and the
 * `public_key_data_from_private_key_data` crypto helper.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class SignerTest {

    // secp256k1 private scalar = 1 (smallest valid key); a deterministic, valid test vector.
    private val privateKeyHex = "0000000000000000000000000000000000000000000000000000000000000001"

    private val nativeLibAvailable: Boolean
        get() = System.getProperty("dash.sdk.lib.dir") != null

    @Before
    fun skipIfNoNativeLib() {
        assumeTrue(
            "Native library not found — run build_platform_local.sh first",
            nativeLibAvailable
        )
    }

    /** Derive the public key for the test scalar and build a populated key store. */
    private fun keyStoreWithTestKey(sdk: DashSDK): Pair<InMemorySigningKeyStore, ByteArray> {
        val pubHex = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, keyType = 0, network = Network.TESTNET)
        val pubBytes = pubHex.hexToBytes()
        val store = InMemorySigningKeyStore()
        store.put(pubBytes, privateKeyHex.hexToBytes())
        return store to pubBytes
    }

    /** `can_sign` returns true for a known key — proves the full callback round-trip works. */
    @Test
    fun canSignReturnsTrueForKnownKey() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val (store, pubBytes) = keyStoreWithTestKey(sdk)
            KeystoreSigner(store, Network.TESTNET).use { signer ->
                assertTrue("can_sign should be true for the stored key", signer.canSign(pubBytes))
            }
        }
    }

    /** `can_sign` returns false for an unknown key — the callback consults the store. */
    @Test
    fun canSignReturnsFalseForUnknownKey() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val (store, _) = keyStoreWithTestKey(sdk)
            KeystoreSigner(store, Network.TESTNET).use { signer ->
                val unknown = ByteArray(33) { 0x02 } // not in the store
                assertFalse("can_sign should be false for an unknown key", signer.canSign(unknown))
            }
        }
    }

    /** The v1 sign primitive produces a non-empty signature over a 32-byte digest. */
    @Test
    fun signProducesSignature() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val (store, pubBytes) = keyStoreWithTestKey(sdk)
            KeystoreSigner(store, Network.TESTNET).use { signer ->
                val digest = ByteArray(32) { it.toByte() }
                val sig = signer.sign(pubBytes, digest)
                assertTrue("signature should be non-empty", sig.isNotEmpty())
            }
        }
    }

    /** Signing for a key the store does not hold surfaces a typed exception, not a crash. */
    @Test
    fun signUnknownKeyThrows() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            KeystoreSigner(InMemorySigningKeyStore(), Network.TESTNET).use { signer ->
                try {
                    signer.sign(ByteArray(33) { 0x02 }, ByteArray(32))
                    fail("Expected DashSDKException for a missing key")
                } catch (e: DashSDKException) {
                    // expected
                }
            }
        }
    }

    /** The crypto helper derives a non-blank public key for a valid private scalar. */
    @Test
    fun publicKeyFromPrivateKeyIsNonBlank() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val pubHex = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET)
            assertTrue("derived public key should be non-blank", pubHex.isNotBlank())
            // Round-trips deterministically for the same scalar.
            assertEquals(pubHex, sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET))
        }
    }

    private fun String.hexToBytes(): ByteArray {
        val clean = removePrefix("0x")
        require(clean.length % 2 == 0) { "hex string must have even length" }
        return ByteArray(clean.length / 2) { i ->
            clean.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
    }
}
