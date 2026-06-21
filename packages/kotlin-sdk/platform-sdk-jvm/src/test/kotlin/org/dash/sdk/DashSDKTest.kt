package org.dash.sdk

import org.dash.sdk.ffi.DashSDKErrorCode
import org.dash.sdk.ffi.DashSDKIdentityInfoNative
import org.dash.sdk.ffi.DashSDKNetwork
import org.dash.sdk.models.Network
import org.junit.Assert.assertEquals
import org.junit.Test

class DashSDKTest {

    @Test
    fun networkEnumValues() {
        assertEquals(DashSDKNetwork.MAINNET, Network.MAINNET.ffiValue)
        assertEquals(DashSDKNetwork.TESTNET, Network.TESTNET.ffiValue)
        assertEquals(DashSDKNetwork.REGTEST, Network.REGTEST.ffiValue)
    }

    @Test
    fun errorCodeConstants() {
        assertEquals(0, DashSDKErrorCode.SUCCESS)
        assertEquals(99, DashSDKErrorCode.INTERNAL_ERROR)
    }

    /**
     * Guards the DashSDKIdentityInfo layout against drift (a wrong offset here is a
     * native crash, not a compile error). C 64-bit:
     * char*(8) + u64(8) + u64(8) + u32(4) + 4 tail padding = 32 bytes.
     * Pure JVM/JNA — no rs-sdk-ffi library needed.
     */
    @Test
    fun identityInfoStructIs32Bytes() {
        assertEquals(32, DashSDKIdentityInfoNative().size())
    }
}
