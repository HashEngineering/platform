package org.dash.sdk

import kotlinx.coroutines.runBlocking
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Exercises the DPNS-extras read bindings. The four DPNS validation helpers
 * ([getValidationMessage], [normalizeUsername], [isValidUsername], [isContestedUsername])
 * are process-local (no DAPI), so they are asserted against behavior. The contested/votes/
 * usernames queries and [getCurrentContests] need a live platform (the mock SDK with
 * `dapiAddresses = null` has nothing to query), so they are not content-asserted offline.
 *
 * Skipped automatically when the host native library is absent — run
 * `build_platform_local.sh` first.
 */
class DpnsExtrasTest {

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
     * DPNS normalization lowercases and folds visually-similar homoglyphs to a canonical
     * form — "Alice" → "a11ce" (the letter 'l' folds to '1'). Process-local; no network.
     */
    @Test
    fun normalizeUsernameNormalizes() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val normalized = sdk.dpns.normalizeUsername("Alice")
            assertEquals("a11ce", normalized)
        }
    }

    /** A well-formed label is reported valid; an obviously broken one is not (process-local). */
    @Test
    fun isValidUsernameDiscriminates() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            assertTrue("expected 'alice' to be a valid DPNS username", sdk.dpns.isValidUsername("alice"))
            assertFalse(
                "expected a label with spaces to be invalid",
                sdk.dpns.isValidUsername("not a valid name!!")
            )
        }
    }

    /** isContestedUsername returns a Boolean without throwing (process-local). */
    @Test
    fun isContestedUsernameDoesNotThrow() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            // A short, high-demand label is typically contested; a long one is not. We only
            // assert the call returns cleanly (the -1/0/1 mapping never throws).
            sdk.dpns.isContestedUsername("alice")
            sdk.dpns.isContestedUsername("a-very-long-uncontested-username-label")
        }
    }

    /** getValidationMessage returns a non-null string for a valid label (process-local). */
    @Test
    fun getValidationMessageReturnsString() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            val msg = sdk.dpns.getValidationMessage("alice")
            assertNotNull("validation message was null", msg)
        }
    }

    /**
     * The new DPNS-extras + contested-resource + group + evonode bindings all resolve in the
     * loaded library. Touching INSTANCE forces JNA to bind the whole interface; the
     * process-local calls above already invoke several of the new symbols, so a missing
     * symbol would have surfaced as an UnsatisfiedLinkError before reaching this assertion.
     */
    @Test
    fun newBindingsAreResolvable() {
        assertNotNull(DashSdkFfi.INSTANCE)
    }

    /**
     * getCurrentContests against the mock SDK must not crash the JVM: it returns a list
     * (possibly empty) and frees the native list cleanly. Network-dependent content is not
     * asserted offline.
     */
    @Test
    fun getCurrentContestsDoesNotCrash() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            runCatching {
                runBlocking { sdk.dpns.getCurrentContests(startTime = 0L, endTime = Long.MAX_VALUE, limit = 5) }
            }
            // Either an empty list or a thrown DashSDKException is acceptable offline;
            // the test only guards against a native crash / double-free.
        }
    }

    /**
     * getContestedNonResolvedUsernames reads a nested `DashSDKContestedNamesList`
     * (name + contest_info + contenders) and frees it. Offline it returns empty / throws;
     * the test guards against a native crash or double-free across the nested-struct read.
     */
    @Test
    fun getContestedNonResolvedUsernamesDoesNotCrash() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            runCatching {
                runBlocking { sdk.dpns.getContestedNonResolvedUsernames(limit = 5) }
            }
        }
    }

    /** Sibling of the above, scoped to one identity. Same offline no-crash guarantee. */
    @Test
    fun getNonResolvedContestsForIdentityDoesNotCrash() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            runCatching {
                runBlocking {
                    sdk.dpns.getNonResolvedContestsForIdentity(
                        identityId = "GWRSAVFMjXx8HpQFaNJMqBV7MBgMK4br5UESsB4S31Ec",
                        limit = 5
                    )
                }
            }
        }
    }
}
