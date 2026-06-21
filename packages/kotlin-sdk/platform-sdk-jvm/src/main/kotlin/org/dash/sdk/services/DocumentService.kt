package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.DashSDKDocumentSearchParamsNative
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.Document

/**
 * High-level service for Dash Platform document operations.
 * Mirrors SwiftDashSDK/DPP/DPPDocument.swift.
 */
class DocumentService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Fetch a single document by its hex-encoded ID.
     *
     * @param contractHandle opaque DataContractHandle from [DataContractService.fetchHandle]
     * @param documentType document type name within the contract
     * @param documentId 32-byte document ID as a hex string
     */
    suspend fun fetchDocument(
        contractHandle: Pointer,
        documentType: String,
        documentId: String
    ): Document = withContext(Dispatchers.IO) {
        val result = ffi.dash_sdk_document_fetch(sdkHandle, contractHandle, documentType, documentId)
        val docHandle = ResultUnwrapper.unwrapHandle(result)
        try {
            parseDocumentHandle(docHandle)
        } finally {
            // Query-path cleanup: dash_sdk_document_destroy(sdkHandle, docHandle) — matches
            // the Swift reference. Returns a DashSDKError* (null on success); free if present.
            ffi.dash_sdk_document_destroy(sdkHandle, docHandle)?.let { ffi.dash_sdk_error_free(it) }
        }
    }

    /**
     * Search documents using optional where/order-by JSON filters.
     *
     * @param contractHandle opaque DataContractHandle
     * @param documentType document type name
     * @param whereJson optional JSON array of where clauses, e.g. `[["name","==","alice"]]`
     * @param orderByJson optional JSON array of order-by clauses
     * @param limit max number of results (0 = server default)
     * @param startAt pagination offset
     * @return list of [Document]
     */
    suspend fun searchDocuments(
        contractHandle: Pointer,
        documentType: String,
        whereJson: String? = null,
        orderByJson: String? = null,
        limit: Int = 0,
        startAt: Int = 0
    ): List<Document> = withContext(Dispatchers.IO) {
        val params = DashSDKDocumentSearchParamsNative().apply {
            data_contract_handle = contractHandle
            this.document_type = documentType
            where_json = whereJson
            order_by_json = orderByJson
            this.limit = limit
            this.start_at = startAt
        }
        val result = ffi.dash_sdk_document_search(sdkHandle, params)
        val json = ResultUnwrapper.unwrapString(result)
        parseDocumentArray(json)
    }

    private fun parseDocumentHandle(handle: Pointer): Document {
        // In the real implementation this would call a dash_sdk_document_get_info() FFI function.
        // Placeholder: return a minimal Document using the handle address as a stand-in ID.
        return Document(
            id = handle.toString(),
            ownerId = "",
            dataContractId = "",
            documentType = "",
            revision = 0L,
            createdAt = 0L,
            updatedAt = 0L,
            propertiesJson = "{}"
        )
    }

    private fun parseDocumentArray(json: String): List<Document> {
        // Minimal parser for the JSON array of documents returned by the search FFI.
        // A full implementation would use kotlinx.serialization or Gson.
        if (json.isBlank() || json == "[]") return emptyList()

        val results = mutableListOf<Document>()
        // Split on top-level object boundaries (simplified; replace with real JSON library).
        val objectPattern = Regex("""\{[^{}]*\}""")
        for (match in objectPattern.findAll(json)) {
            val obj = match.value
            results += Document(
                id = jsonField(obj, "id"),
                ownerId = jsonField(obj, "owner_id"),
                dataContractId = jsonField(obj, "data_contract_id"),
                documentType = jsonField(obj, "document_type"),
                revision = jsonLong(obj, "revision"),
                createdAt = jsonLong(obj, "created_at"),
                updatedAt = jsonLong(obj, "updated_at"),
                propertiesJson = obj
            )
        }
        return results
    }

    private fun jsonField(json: String, key: String): String =
        Regex(""""$key"\s*:\s*"([^"]+)"""").find(json)?.groupValues?.get(1) ?: ""

    private fun jsonLong(json: String, key: String): Long =
        Regex(""""$key"\s*:\s*(\d+)""").find(json)?.groupValues?.get(1)?.toLongOrNull() ?: 0L
}
