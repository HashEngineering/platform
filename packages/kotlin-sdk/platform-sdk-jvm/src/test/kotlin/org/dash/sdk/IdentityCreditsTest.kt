package org.dash.sdk

import com.sun.jna.Pointer
import kotlinx.coroutines.runBlocking
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.IdentityPublicKeyParams
import org.dash.sdk.models.Network
import org.dash.sdk.models.PutSettings
import org.dash.sdk.signing.InMemorySigningKeyStore
import org.dash.sdk.signing.KeystoreSigner
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Phase-D credit operations (top-up / transfer / withdraw) plus the [PutSettings] struct.
 *
 * As with registration, an end-to-end run needs a live node, so these assert the marshalling
 * path rather than success: the relevant FFI entry points validate their inputs (asset-lock
 * proof, recipient id, withdrawal address) before any network I/O, so malformed inputs surface
 * a typed [DashSDKException] (never a crash) — which pins the argument marshalling, including
 * the 48-byte [PutSettings] struct, offline.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class IdentityCreditsTest {

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

    private fun pubBytes(sdk: DashSDK): ByteArray =
        sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()

    private fun signer(sdk: DashSDK): KeystoreSigner {
        val store = InMemorySigningKeyStore()
        store.put(pubBytes(sdk), privateKeyHex.hexToBytes())
        return KeystoreSigner(store, Network.TESTNET)
    }

    private fun identityHandle(sdk: DashSDK): Pointer = runBlocking {
        sdk.identity.createFromComponents(
            ByteArray(32) { 0x07 },
            listOf(IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes(sdk)))
        )
    }

    /** Top-up marshals: a malformed asset-lock proof throws rather than crashing. */
    @Test
    fun topUpWithMalformedProofThrows() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val handle = identityHandle(sdk)
            try {
                sdk.identity.topUpWithInstantLock(
                    identityHandle = handle,
                    instantLockBytes = ByteArray(64) { it.toByte() },
                    transactionBytes = ByteArray(64) { (it * 2).toByte() },
                    outputIndex = 0,
                    assetLockPrivateKey = privateKeyHex.hexToBytes(),
                )
                fail("Expected DashSDKException for a malformed asset-lock proof")
            } catch (e: DashSDKException) {
                // expected
            } finally {
                sdk.identity.destroyIdentity(handle)
            }
        }
    }

    /** Transfer marshals: an invalid recipient id throws (parsed before any network I/O). */
    @Test
    fun transferWithInvalidRecipientThrows() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val handle = identityHandle(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.identity.transferCredits(
                        fromIdentityHandle = handle,
                        toIdentityId = "not a valid base58 identity id !!!",
                        amount = 1000uL,
                        publicKeyId = 0,
                        signer = signer,
                    )
                    fail("Expected DashSDKException for an invalid recipient id")
                } catch (e: DashSDKException) {
                    // expected
                } finally {
                    sdk.identity.destroyIdentity(handle)
                }
            }
        }
    }

    /** Withdraw marshals: an invalid address throws (parsed before any network I/O). */
    @Test
    fun withdrawWithInvalidAddressThrows() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val handle = identityHandle(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.identity.withdraw(
                        identityHandle = handle,
                        address = "not a valid dash address !!!",
                        amount = 1000uL,
                        coreFeePerByte = 0,
                        publicKeyId = 0,
                        signer = signer,
                    )
                    fail("Expected DashSDKException for an invalid address")
                } catch (e: DashSDKException) {
                    // expected
                } finally {
                    sdk.identity.destroyIdentity(handle)
                }
            }
        }
    }

    /**
     * A custom [PutSettings] passes through the FFI without corrupting the call: registration
     * with tuned settings still fails on the (malformed) proof rather than crashing, which
     * exercises the 48-byte settings struct marshalling.
     */
    @Test
    fun customPutSettingsMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val store = InMemorySigningKeyStore().apply { put(pubBytes(sdk), privateKeyHex.hexToBytes()) }
            KeystoreSigner(store, Network.TESTNET).use { signer ->
                val keys = listOf(IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pubBytes(sdk)))
                val settings = PutSettings(
                    connectTimeoutMs = 5000,
                    timeoutMs = 10_000,
                    retries = 3,
                    banFailedAddress = true,
                    userFeeIncrease = 10,
                    waitTimeoutMs = 30_000,
                )
                try {
                    sdk.identity.registerWithInstantLock(
                        publicKeys = keys,
                        instantLockBytes = ByteArray(64) { it.toByte() },
                        transactionBytes = ByteArray(64) { (it * 2).toByte() },
                        outputIndex = 0,
                        assetLockPrivateKey = privateKeyHex.hexToBytes(),
                        signer = signer,
                        settings = settings,
                    )
                    fail("Expected DashSDKException for a malformed asset-lock proof")
                } catch (e: DashSDKException) {
                    // expected — the settings struct marshalled fine; the proof is what failed
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
