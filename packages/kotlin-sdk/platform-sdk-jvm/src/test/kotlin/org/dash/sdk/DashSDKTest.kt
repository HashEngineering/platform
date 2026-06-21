package org.dash.sdk

import org.dash.sdk.ffi.DashSDKDataContractFetchResultNative
import org.dash.sdk.ffi.DashSDKDocumentInfoNative
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

    /**
     * Guards the DashSDKDocumentInfo layout against drift. C 64-bit (all 8-aligned):
     * id*(8) + owner_id*(8) + data_contract_id*(8) + document_type*(8)
     * + revision u64(8) + created_at i64(8) + updated_at i64(8)
     * + data_fields_count uintptr(8) + data_fields*(8) = 72 bytes, no tail padding.
     * Pure JVM/JNA — no rs-sdk-ffi library needed.
     */
    @Test
    fun documentInfoStructIs72Bytes() {
        assertEquals(72, DashSDKDocumentInfoNative().size())
    }

    /**
     * Guards the DashSDKDataContractFetchResult layout against drift. C 64-bit (all 8-aligned):
     * contract_handle*(8) + json_string*(8) + serialized_data*(8)
     * + serialized_data_len uintptr(8) + error*(8) = 40 bytes, no tail padding.
     * Pure JVM/JNA — no rs-sdk-ffi library needed.
     */
    @Test
    fun dataContractFetchResultStructIs40Bytes() {
        assertEquals(40, DashSDKDataContractFetchResultNative().size())
    }
}
