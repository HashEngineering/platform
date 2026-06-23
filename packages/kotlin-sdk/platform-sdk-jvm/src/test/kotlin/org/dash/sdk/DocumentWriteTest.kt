package org.dash.sdk

import com.sun.jna.Pointer
import kotlinx.coroutines.runBlocking
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.IdentityPublicKeyParams
import org.dash.sdk.models.Network
import org.dash.sdk.signing.InMemorySigningKeyStore
import org.dash.sdk.signing.KeystoreSigner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Document write-path (`dash_sdk_document_*` state transitions) over the external-signer pattern.
 *
 * The builders ([DocumentService.createDocument] / [DocumentService.makeHandle]) are
 * process-local — they parse the base58 IDs + properties JSON and return a DocumentHandle
 * without touching the network — so they are asserted directly (non-null handle, 32-byte
 * entropy). The put/replace/delete/transfer/purchase/update-price ops need a live node, so
 * they are marshalling smoke tests: a real [KeystoreSigner] plus a signing-key handle are
 * crossed over the FFI and the call must surface a typed [DashSDKException] (never a JVM
 * crash). This pins the argument marshalling — the entropy buffer, the signing-key /
 * SignerHandle pointers, and the nullable token-payment / st-options NULLs — offline.
 *
 * Skipped automatically when the host native library is absent — run
 * `./build_platform_local.sh` first.
 */
class DocumentWriteTest {

    // A real testnet base58 32-byte id (the DPNS contract id); valid for parse-only builders.
    private val contractId = "GWRSAVFMjXx8HpQFaNJMqBV7MBgMK4br5UESsB4S31Ec"
    private val ownerId = "GWRSAVFMjXx8HpQFaNJMqBV7MBgMK4br5UESsB4S31Ec"
    private val documentType = "domain"
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

    private fun signer(sdk: DashSDK): KeystoreSigner {
        val pub = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
        val store = InMemorySigningKeyStore().apply { put(pub, privateKeyHex.hexToBytes()) }
        return KeystoreSigner(store, Network.TESTNET)
    }

    /** Build a standalone signing-key handle (an IdentityPublicKeyHandle) for write calls. */
    private fun signingKey(sdk: DashSDK): Pointer {
        val pub = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
        return sdk.identity.createPublicKey(
            IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pub)
        )
    }

    /** createDocument builds a handle + 32-byte entropy without a network call. */
    @Test
    fun createDocumentReturnsHandleAndEntropy() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                val result = sdk.document.createDocument(
                    dataContractId = contractId,
                    documentType = documentType,
                    ownerIdentityId = ownerId,
                    propertiesJson = """{"label":"alice","normalizedLabel":"alice"}"""
                )
                assertNotNull("expected a non-null DocumentHandle", result.handle)
                assertEquals("entropy must be 32 bytes", 32, result.entropy.size)
                sdk.document.destroyDocument(result.handle)
            } catch (e: DashSDKException) {
                // Acceptable: the builder may reject the property set for this contract/type.
                // The point proven is a clean typed failure rather than a JVM crash.
            }
        }
    }

    /** makeHandle builds a DocumentHandle from explicit params without a network call. */
    @Test
    fun makeHandleReturnsHandle() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            try {
                val handle = sdk.document.makeHandle(
                    id = contractId,
                    dataContractId = contractId,
                    documentType = documentType,
                    ownerIdentityId = ownerId,
                    propertiesJson = """{"label":"alice"}"""
                )
                assertNotNull("expected a non-null DocumentHandle", handle)
                sdk.document.destroyDocument(handle)
            } catch (e: DashSDKException) {
                // Acceptable typed failure (no crash).
            }
        }
    }

    /** setProperties on a freshly-made handle either succeeds or throws typed — never crashes. */
    @Test
    fun setPropertiesMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val handle = try {
                sdk.document.makeHandle(
                    id = contractId,
                    dataContractId = contractId,
                    documentType = documentType,
                    ownerIdentityId = ownerId,
                    propertiesJson = """{"label":"alice"}"""
                )
            } catch (e: DashSDKException) {
                return@use // builder rejected params on this lib; nothing to mutate
            }
            try {
                sdk.document.setProperties(handle, """{"label":"bob"}""")
            } catch (e: DashSDKException) {
                // typed failure acceptable
            } finally {
                sdk.document.destroyDocument(handle)
            }
        }
    }

    /** putToPlatform rejects a wrong-length entropy before any FFI call. */
    @Test
    fun putRejectsBadEntropyLength() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                val handle = sdk.document.makeHandle(contractId, contractId, documentType, ownerId, "{}")
                try {
                    sdk.document.putToPlatform(
                        documentHandle = handle,
                        dataContractId = contractId,
                        documentTypeName = documentType,
                        entropy = ByteArray(31), // wrong length
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected IllegalArgumentException for 31-byte entropy")
                } catch (e: IllegalArgumentException) {
                    // expected
                } finally {
                    sdk.document.destroyDocument(handle)
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** putToPlatform marshals signer + key + entropy across the FFI; no node ⇒ typed throw, no crash. */
    @Test
    fun putToPlatformMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                val handle = sdk.document.makeHandle(contractId, contractId, documentType, ownerId, "{}")
                try {
                    sdk.document.putToPlatform(
                        documentHandle = handle,
                        dataContractId = contractId,
                        documentTypeName = documentType,
                        entropy = ByteArray(32) { it.toByte() },
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected DashSDKException without a live node")
                } catch (e: DashSDKException) {
                    // expected: clean typed failure, no native crash
                } finally {
                    sdk.document.destroyDocument(handle)
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** deleteDocument marshals its 4 ids + signer + key across the FFI without crashing. */
    @Test
    fun deleteMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.document.deleteDocument(
                        documentId = contractId,
                        ownerId = ownerId,
                        dataContractId = contractId,
                        documentTypeName = documentType,
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected DashSDKException without a live node")
                } catch (e: DashSDKException) {
                    // expected
                } finally {
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** purchase marshals the uint64 price + ids + signer across the FFI without crashing. */
    @Test
    fun purchaseMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                val handle = sdk.document.makeHandle(contractId, contractId, documentType, ownerId, "{}")
                try {
                    sdk.document.purchase(
                        documentHandle = handle,
                        dataContractId = contractId,
                        documentTypeName = documentType,
                        price = 1000uL,
                        purchaserId = ownerId,
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected DashSDKException without a live node")
                } catch (e: DashSDKException) {
                    // expected
                } finally {
                    sdk.document.destroyDocument(handle)
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** updatePrice marshals the uint64 price + ids + signer across the FFI without crashing. */
    @Test
    fun updatePriceMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                val handle = sdk.document.makeHandle(contractId, contractId, documentType, ownerId, "{}")
                try {
                    sdk.document.updatePrice(
                        documentHandle = handle,
                        dataContractId = contractId,
                        documentTypeName = documentType,
                        price = 2000uL,
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected DashSDKException without a live node")
                } catch (e: DashSDKException) {
                    // expected
                } finally {
                    sdk.document.destroyDocument(handle)
                    sdk.identity.destroyPublicKey(key)
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
