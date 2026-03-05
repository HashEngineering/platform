package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.DataContract

/**
 * High-level service for Dash Platform data contract operations.
 * Mirrors SwiftDashSDK/DataContract.swift.
 */
class DataContractService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Fetch a data contract as a JSON string.
     *
     * @param contractId 32-byte contract ID as a hex string
     * @return [DataContract] containing the contract ID and its JSON representation
     */
    suspend fun fetchDataContract(contractId: String): DataContract = withContext(Dispatchers.IO) {
        val result = ffi.dash_sdk_data_contract_fetch_json(sdkHandle, contractId)
        val json = ResultUnwrapper.unwrapString(result)
        DataContract(id = contractId, json = json)
    }

    /**
     * Fetch and hold an opaque DataContractHandle for use in document queries.
     * The caller is responsible for calling [releaseHandle] when done.
     *
     * @param contractId 32-byte contract ID as a hex string
     * @return opaque native handle pointer
     */
    suspend fun fetchHandle(contractId: String): Pointer = withContext(Dispatchers.IO) {
        val result = ffi.dash_sdk_data_contract_fetch(sdkHandle, contractId)
        ResultUnwrapper.unwrapHandle(result)
    }

    /** Release a DataContractHandle obtained from [fetchHandle]. */
    fun releaseHandle(handle: Pointer) {
        ffi.dash_sdk_data_contract_handle_free(handle)
    }
}
