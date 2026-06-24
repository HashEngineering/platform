package org.dash.sdk

import kotlinx.coroutines.runBlocking
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.IdentityPublicKeyParams
import org.dash.sdk.models.Network
import org.dash.sdk.signing.InMemorySigningKeyStore
import org.dash.sdk.signing.KeystoreSigner
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the DPNS username-registration write path (`dpns_register_name`).
 *
 * A real registration needs a live node, so this pins the marshalling: build an identity
 * handle + signing-key handle + signer, call registerName, and require a typed
 * [DashSDKException] (never a JVM crash) when it can't reach a node.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class DpnsRegisterTest {

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

    @Test
    fun registerNameMarshalsAndThrowsWithoutNode() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val pubBytes = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
            val store = InMemorySigningKeyStore().apply { put(pubBytes, privateKeyHex.hexToBytes()) }
            KeystoreSigner(store, Network.TESTNET).use { signer ->
                val identityHandle = sdk.identity.createFromComponents(
                    ByteArray(32) { 0x07 },
                    listOf(IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes))
                )
                val keyHandle = sdk.identity.getPublicKeyById(identityHandle, 0)
                try {
                    sdk.dpns.registerName(
                        label = "alice",
                        identityHandle = identityHandle,
                        signingKeyHandle = keyHandle,
                        signer = signer,
                    )
                    fail("Expected DashSDKException registering DPNS name without a node")
                } catch (e: DashSDKException) {
                    // expected: typed failure, no native crash — marshalling is what we pin
                } finally {
                    sdk.identity.destroyPublicKey(keyHandle)
                    sdk.identity.destroyIdentity(identityHandle)
                }
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
