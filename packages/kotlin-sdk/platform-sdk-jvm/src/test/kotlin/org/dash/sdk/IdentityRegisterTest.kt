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
 * Exercises Phase-C identity registration (`put_to_platform_with_instant_lock_and_wait`).
 *
 * A real end-to-end registration needs a funded asset lock and a live node, so this does
 * not assert a successful broadcast. Instead it verifies the marshalling path: building the
 * identity, packing the asset-lock proof + key, passing the signer handle, and crossing the
 * FFI. `put_to_platform_with_instant_lock_and_wait` parses the asset-lock proof before any
 * network I/O, so a malformed proof surfaces a typed [DashSDKException] (never a JVM crash),
 * which is exactly the marshalling guarantee we want to pin down.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class IdentityRegisterTest {

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

    /** Build a signer holding the test key, plus the matching single-key params list. */
    private fun signerAndKeys(sdk: DashSDK): Pair<KeystoreSigner, List<IdentityPublicKeyParams>> {
        val pubBytes = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
        val store = InMemorySigningKeyStore()
        store.put(pubBytes, privateKeyHex.hexToBytes())
        val keys = listOf(IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes))
        return KeystoreSigner(store, Network.TESTNET) to keys
    }

    /** A wrong-length asset-lock key is rejected before any FFI call. */
    @Test
    fun registerRejectsBadAssetLockKeyLength() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val (signer, keys) = signerAndKeys(sdk)
            signer.use {
                try {
                    sdk.identity.registerWithInstantLock(
                        publicKeys = keys,
                        instantLockBytes = ByteArray(32) { 1 },
                        transactionBytes = ByteArray(32) { 2 },
                        outputIndex = 0,
                        assetLockPrivateKey = ByteArray(31), // wrong length
                        signer = signer,
                    )
                    fail("Expected IllegalArgumentException for a 31-byte asset-lock key")
                } catch (e: IllegalArgumentException) {
                    // expected
                }
            }
        }
    }

    /**
     * A malformed asset-lock proof surfaces a typed [DashSDKException] rather than crashing —
     * proving the build-identity -> pack-proof -> pass-signer -> FFI path marshals correctly.
     */
    @Test
    fun registerWithMalformedProofThrowsNotCrashes() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val (signer, keys) = signerAndKeys(sdk)
            signer.use {
                try {
                    sdk.identity.registerWithInstantLock(
                        publicKeys = keys,
                        instantLockBytes = ByteArray(64) { it.toByte() },
                        transactionBytes = ByteArray(64) { (it * 3).toByte() },
                        outputIndex = 0,
                        assetLockPrivateKey = privateKeyHex.hexToBytes(),
                        signer = signer,
                    )
                    fail("Expected DashSDKException for a malformed asset-lock proof")
                } catch (e: DashSDKException) {
                    // expected: clean typed failure, no native crash
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
