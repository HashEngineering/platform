package org.dash.sdk

import org.dash.sdk.ffi.DashSDKErrorCode
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
}
