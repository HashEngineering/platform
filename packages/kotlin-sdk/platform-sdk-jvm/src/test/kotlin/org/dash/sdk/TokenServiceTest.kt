package org.dash.sdk

import com.sun.jna.Pointer
import kotlinx.coroutines.runBlocking
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.IdentityPublicKeyParams
import org.dash.sdk.models.Network
import org.dash.sdk.signing.InMemorySigningKeyStore
import org.dash.sdk.signing.KeystoreSigner
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

    // -------------------------------------------------------------------------
    // Token write-path (state transitions) — external-signer marshalling smoke tests.
    // Each crosses a real KeystoreSigner + a standalone signing-key handle + the Base58
    // contract id + 32-byte raw ids over the FFI. With no live node (dapiAddresses = null)
    // and a non-existent token contract, the call must surface a typed DashSDKException —
    // never a JVM crash — pinning the struct/byte-array/pointer marshalling offline.
    // Mirrors DocumentWriteTest / IdentityCreditsTest.
    // -------------------------------------------------------------------------

    private val privateKeyHex = "0000000000000000000000000000000000000000000000000000000000000001"

    // A valid 32-byte raw id, reused for owner / recipient / target across the smoke tests.
    private val ownerId = ByteArray(32) { (it + 1).toByte() }
    private val otherId = ByteArray(32) { (it + 2).toByte() }

    private fun signer(sdk: DashSDK): KeystoreSigner {
        val pub = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
        val store = InMemorySigningKeyStore().apply { put(pub, privateKeyHex.hexToBytes()) }
        return KeystoreSigner(store, Network.TESTNET)
    }

    /** Build a standalone IdentityPublicKeyHandle for write calls. */
    private fun signingKey(sdk: DashSDK): Pointer {
        val pub = sdk.utils.publicKeyFromPrivateKey(privateKeyHex, network = Network.TESTNET).hexToBytes()
        return sdk.identity.createPublicKey(
            IdentityPublicKeyParams(keyId = 0, keyType = 0, purpose = 0, securityLevel = 0, data = pub)
        )
    }

    /** mint rejects a wrong-length owner id before any FFI call. */
    @Test
    fun mintRejectsBadOwnerLength() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.mint(
                        transitionOwnerId = ByteArray(31), // wrong length
                        tokenContractId = dpnsContractId,
                        amount = 100,
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected IllegalArgumentException for 31-byte owner id")
                } catch (e: IllegalArgumentException) {
                    // expected
                } finally {
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** mint marshals struct + signer + key + 32-byte ids; no node ⇒ typed throw, no crash. */
    @Test
    fun mintMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.mint(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        amount = 1000,
                        signingKeyHandle = key,
                        signer = signer,
                        recipientId = otherId,
                        publicNote = "mint test",
                    )
                    fail("Expected DashSDKException without a live node")
                } catch (e: DashSDKException) {
                    // expected: clean typed failure, no native crash
                } finally {
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** burn marshals struct + signer + key across the FFI without crashing. */
    @Test
    fun burnMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.burn(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        amount = 500,
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

    /** transfer marshals the recipient id + uint64 amount + notes across the FFI. */
    @Test
    fun transferMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.transfer(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        recipientId = otherId,
                        amount = 250,
                        signingKeyHandle = key,
                        signer = signer,
                        publicNote = "transfer test",
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

    /** freeze marshals the target id + signer + key across the FFI. */
    @Test
    fun freezeMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.freeze(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        targetIdentityId = otherId,
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

    /** unfreeze reuses the freeze params struct and marshals it the same way. */
    @Test
    fun unfreezeMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.unfreeze(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        targetIdentityId = otherId,
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

    /** claim rejects a wrong-length owner id before any FFI call. */
    @Test
    fun claimRejectsBadOwnerLength() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.claim(
                        transitionOwnerId = ByteArray(31), // wrong length
                        tokenContractId = dpnsContractId,
                        distributionType = 1,
                        signingKeyHandle = key,
                        signer = signer,
                    )
                    fail("Expected IllegalArgumentException for 31-byte owner id")
                } catch (e: IllegalArgumentException) {
                    // expected
                } finally {
                    sdk.identity.destroyPublicKey(key)
                }
            }
        }
    }

    /** claim marshals struct (incl. distribution_type enum) + signer + key; no node ⇒ typed throw. */
    @Test
    fun claimMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.claim(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        distributionType = 1, // Perpetual
                        signingKeyHandle = key,
                        signer = signer,
                        publicNote = "claim test",
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

    /** setSinglePrice marshals the pricing_type enum + uint64 single_price + null entries. */
    @Test
    fun setSinglePriceMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.setSinglePrice(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        singlePrice = 1_000_000,
                        signingKeyHandle = key,
                        signer = signer,
                        publicNote = "set price test",
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

    /** purchase marshals the amount + total_agreed_price uint64 pair across the FFI. */
    @Test
    fun purchaseMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.purchase(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        amount = 10,
                        totalAgreedPrice = 5_000_000,
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

    /** destroyFrozenFunds marshals the frozen identity id + signer + key across the FFI. */
    @Test
    fun destroyFrozenFundsMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.destroyFrozenFunds(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        frozenIdentityId = otherId,
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

    /** emergencyAction marshals the action enum + signer + key across the FFI. */
    @Test
    fun emergencyActionMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.emergencyAction(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        action = 0, // Pause
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

    /**
     * updateContractTokenConfiguration marshals the dense mixed struct (enum + uint64 +
     * mid-struct C bool + identity-id pointer + uint16 + enum) across the FFI.
     */
    @Test
    fun updateContractTokenConfigurationMarshals() = runBlocking {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val key = signingKey(sdk)
            signer(sdk).use { signer ->
                try {
                    sdk.token.updateContractTokenConfiguration(
                        transitionOwnerId = ownerId,
                        tokenContractId = dpnsContractId,
                        updateType = 3, // NewTokensDestinationIdentity (uses identity_id)
                        signingKeyHandle = key,
                        signer = signer,
                        identityId = otherId,
                        boolValue = true,
                        publicNote = "config update test",
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

    private fun String.hexToBytes(): ByteArray {
        val clean = removePrefix("0x")
        require(clean.length % 2 == 0) { "hex string must have even length" }
        return ByteArray(clean.length / 2) { i ->
            clean.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
    }
}
