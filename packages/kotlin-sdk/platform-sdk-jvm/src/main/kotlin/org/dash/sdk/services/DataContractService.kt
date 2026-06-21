package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSDKErrorCode
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.DataContract
import org.dash.sdk.models.DataContractFetchResult
import org.dash.sdk.models.DashSDKException

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
        ffi.dash_sdk_data_contract_destroy(handle)
    }

    /**
     * Fetch a data contract by ID, optionally returning its JSON string and/or serialized
     * bytes. Wraps `dash_sdk_data_contract_fetch_with_serialization`, which returns a
     * result struct by value carrying its own error/handle/buffers.
     *
     * The contract handle inside the result is freed along with the other inner buffers via
     * `dash_sdk_data_contract_fetch_result_free` (this method does not surface the handle).
     *
     * @param contractId 32-byte contract ID as a hex string
     * @param returnJson request the JSON representation
     * @param returnSerialized request the serialized bytes
     * @return a [DataContractFetchResult] with whichever fields were requested
     * @throws DashSDKException if the FFI reports an error
     */
    suspend fun fetchWithSerialization(
        contractId: String,
        returnJson: Boolean,
        returnSerialized: Boolean
    ): DataContractFetchResult = withContext(Dispatchers.IO) {
        val result = ffi.dash_sdk_data_contract_fetch_with_serialization(
            sdkHandle, contractId, returnJson, returnSerialized
        )
        try {
            val errorPtr = result.error
            if (errorPtr != null) {
                // Same DashSDKError shape ResultUnwrapper reads: int code @0, char* message @POINTER_SIZE.
                val code = errorPtr.getInt(0)
                val msgPtr = errorPtr.getPointer(com.sun.jna.Native.POINTER_SIZE.toLong())
                val msg = msgPtr?.getString(0) ?: "Unknown error"
                throw DashSDKException(code, msg)
            }
            val json = result.json_string?.getString(0, "UTF-8")
            val serialized = result.serialized_data?.let { ptr ->
                ptr.getByteArray(0, result.serialized_data_len.toInt())
            }
            DataContractFetchResult(id = contractId, json = json, serialized = serialized)
        } finally {
            // Free the inner heap pointers (json_string, serialized_data, contract_handle,
            // error). The outer struct lives on our stack (returned by value); pass its
            // backing pointer. Reading the struct's pointer requires the native fields to be
            // laid down — write() ensures the JNA-side memory mirrors the returned struct.
            result.write()
            ffi.dash_sdk_data_contract_fetch_result_free(result.pointer)
        }
    }

    /**
     * Fetch a contract's revision history as a JSON string.
     *
     * @param startAtMs only entries at/after this timestamp (ms); 0 = from genesis
     */
    suspend fun fetchHistory(
        contractId: String,
        limit: Int = 0,
        offset: Int = 0,
        startAtMs: Long = 0L
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_data_contract_fetch_history(sdkHandle, contractId, limit, offset, startAtMs)
        )
    }

    /**
     * Get the JSON schema for a document type from a DataContractHandle.
     *
     * @param contractHandle handle from [fetchHandle]
     * @param documentType document type name within the contract
     * @return the schema JSON, or null if the type is unknown / handle invalid
     */
    suspend fun getSchema(contractHandle: Pointer, documentType: String): String? =
        withContext(Dispatchers.IO) {
            val ptr = ffi.dash_sdk_data_contract_get_schema(contractHandle, documentType)
                ?: return@withContext null
            try {
                ptr.getString(0, "UTF-8")
            } finally {
                ffi.dash_sdk_string_free(ptr)
            }
        }
}
