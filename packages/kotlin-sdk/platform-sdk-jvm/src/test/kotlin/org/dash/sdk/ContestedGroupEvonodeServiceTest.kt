package org.dash.sdk

import org.dash.sdk.models.Network
import org.junit.Assert.assertNotNull
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Link/wiring checks for the new [org.dash.sdk.services.ContestedResourceService],
 * [org.dash.sdk.services.GroupService], and [org.dash.sdk.services.EvonodeService] services.
 *
 * All of their methods are network queries (they take the SDK handle and hit DAPI), so they
 * cannot be content-asserted against the mock SDK (`dapiAddresses = null`). These tests only
 * confirm the services are reachable off the [DashSDK] facade and constructed without error;
 * the underlying FFI symbols would fail to bind (UnsatisfiedLinkError) on first use if absent,
 * which is covered by the actual invocations in [DpnsExtrasTest].
 *
 * Skipped automatically when the host native library is absent — run
 * `build_platform_local.sh` first.
 */
class ContestedGroupEvonodeServiceTest {

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
    fun newServicesAreWiredOntoFacade() {
        DashSDK.create(network = Network.TESTNET, dapiAddresses = null).use { sdk ->
            assertNotNull("contestedResource service missing", sdk.contestedResource)
            assertNotNull("group service missing", sdk.group)
            assertNotNull("evonode service missing", sdk.evonode)
        }
    }
}
