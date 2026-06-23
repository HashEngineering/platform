package org.dash.sdk

import kotlinx.coroutines.runBlocking
import org.dash.sdk.models.IdentityPublicKeyParams
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises Phase-B identity-object construction offline (real native calls, no node):
 * build public keys from raw components and assemble an identity handle via
 * `create_from_components`, then read it back through `identity_get_info`.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class IdentityCreateTest {

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

    /** A standalone public key built from components round-trips its key ID. */
    @Test
    fun createPublicKeyRoundTripsId() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val pubBytes = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
            val keyHandle = sdk.identity.createPublicKey(
                IdentityPublicKeyParams(
                    keyId = 5, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes
                )
            )
            try {
                assertEquals(5, sdk.identity.getPublicKeyId(keyHandle))
            } finally {
                sdk.identity.destroyPublicKey(keyHandle)
            }
        }
    }

    /** Assemble an identity from components, then read it back via get_info. */
    @Test
    fun createFromComponentsRoundTrips() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val pubBytes = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
            // Placeholder id (the real id would be derived from the asset-lock proof at registration).
            val placeholderId = ByteArray(32) { (it + 1).toByte() }
            val masterAuthKey = IdentityPublicKeyParams(
                keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes
            )

            val handle = sdk.identity.createFromComponents(placeholderId, listOf(masterAuthKey))
            try {
                assertNotNull(handle)
                val info = sdk.identity.getInfo(handle)
                assertEquals("one public key expected", 1, info.publicKeysCount)
                assertEquals("balance should be 0 for a fresh identity", 0L, info.balance)
                assertTrue("id should be non-blank", info.id.isNotBlank())
            } finally {
                sdk.identity.destroyIdentity(handle)
            }
        }
    }

    /** A second key with a distinct ID is reflected in the identity's key count. */
    @Test
    fun createFromComponentsWithTwoKeys() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val pubBytes = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
            val placeholderId = ByteArray(32) { 0x07 }
            val keys = listOf(
                IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes),
                IdentityPublicKeyParams(keyId = 1, keyType = 0, purpose = 0, securityLevel = 2, data = pubBytes),
            )
            val handle = sdk.identity.createFromComponents(placeholderId, keys)
            try {
                assertEquals(2, sdk.identity.getInfo(handle).publicKeysCount)
            } finally {
                sdk.identity.destroyIdentity(handle)
            }
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
