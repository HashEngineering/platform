package org.dash.sdk.services

import com.sun.jna.Memory
import com.sun.jna.Pointer
import java.lang.ref.Reference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSDKDocumentCreateParamsNative
import org.dash.sdk.ffi.DashSDKDocumentCreateResultNative
import org.dash.sdk.ffi.DashSDKDocumentHandleParamsNative
import org.dash.sdk.ffi.DashSDKDocumentInfoNative
import org.dash.sdk.ffi.DashSDKDocumentSearchParamsNative
import org.dash.sdk.ffi.DashSDKErrorCode
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.ffi.toNative
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Document
import org.dash.sdk.models.DocumentCreateResult
import org.dash.sdk.models.PutSettings
import org.dash.sdk.signing.Signer

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

    // -------------------------------------------------------------------------
    // Document write-path (state transitions) — external-signer pattern. Each is a
    // 1:1 FFI wrapper: marshal in → call → unwrap. The signing key is an
    // IdentityPublicKeyHandle [Pointer] (from IdentityService.getPublicKeyById /
    // getSigningKeyForTransition); the [signer] supplies the SignerHandle. The optional
    // token_payment_info and state_transition_creation_options are always NULL.
    // -------------------------------------------------------------------------

    /**
     * Create a document (process-local; builds a DocumentHandle, no broadcast).
     *
     * @param dataContractId data contract ID (base58)
     * @param documentType document type name within the contract
     * @param ownerIdentityId owner identity ID (base58)
     * @param propertiesJson JSON object of the document's properties
     * @return the created [DocumentCreateResult] (handle + 32-byte entropy); the caller owns
     *   the handle and must free it with [destroyDocument]
     */
    suspend fun createDocument(
        dataContractId: String,
        documentType: String,
        ownerIdentityId: String,
        propertiesJson: String
    ): DocumentCreateResult = withContext(Dispatchers.IO) {
        val params = DashSDKDocumentCreateParamsNative().apply {
            data_contract_id = dataContractId
            document_type = documentType
            owner_identity_id = ownerIdentityId
            properties_json = propertiesJson
            write()
        }
        val dataPtr = ResultUnwrapper.unwrap(ffi.dash_sdk_document_create(sdkHandle, params))
        Reference.reachabilityFence(params)
        try {
            val native = DashSDKDocumentCreateResultNative(dataPtr).apply { read() }
            val handle = native.document_handle
                ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "document_create returned a null handle")
            DocumentCreateResult(handle = handle, entropy = native.entropy.copyOf())
        } finally {
            ffi.dash_sdk_document_create_result_free(dataPtr)
        }
    }

    /**
     * Build a DocumentHandle from explicit parameters (process-local; no broadcast).
     *
     * @param id document ID (base58)
     * @param dataContractId data contract ID (base58)
     * @param documentType document type name
     * @param ownerIdentityId owner identity ID (base58)
     * @param propertiesJson JSON object of the document's properties
     * @param revision optional revision (0 = no revision)
     * @return an opaque DocumentHandle; the caller owns it and must free it with [destroyDocument]
     */
    suspend fun makeHandle(
        id: String,
        dataContractId: String,
        documentType: String,
        ownerIdentityId: String,
        propertiesJson: String,
        revision: Long = 0
    ): Pointer = withContext(Dispatchers.IO) {
        val params = DashSDKDocumentHandleParamsNative().apply {
            this.id = id
            data_contract_id = dataContractId
            document_type = documentType
            owner_identity_id = ownerIdentityId
            properties_json = propertiesJson
            this.revision = revision
            write()
        }
        val handle = ResultUnwrapper.unwrapHandle(ffi.dash_sdk_document_make_handle(params))
        Reference.reachabilityFence(params)
        handle
    }

    /**
     * Set a DocumentHandle's properties from a JSON object (process-local mutation).
     *
     * @param documentHandle handle to mutate
     * @param propertiesJson JSON object of the new properties
     * @throws DashSDKException if the FFI reports an error
     */
    suspend fun setProperties(documentHandle: Pointer, propertiesJson: String): Unit =
        withContext(Dispatchers.IO) {
            val errorPtr = ffi.dash_sdk_document_set_properties(documentHandle, propertiesJson)
            if (errorPtr != null) {
                val code = errorPtr.getInt(0)
                val msgPtr = errorPtr.getPointer(com.sun.jna.Native.POINTER_SIZE.toLong())
                val msg = msgPtr?.getString(0) ?: "Unknown error"
                ffi.dash_sdk_error_free(errorPtr)
                throw DashSDKException(code, msg)
            }
        }

    /**
     * Put a document to platform and wait for confirmation. Network call — requires a live node.
     *
     * @param documentHandle the document to create (e.g. from [createDocument])
     * @param dataContractId data contract ID (base58)
     * @param documentTypeName document type name
     * @param entropy 32-byte entropy from [createDocument] (the document-ID entropy)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the confirmed DocumentHandle; the caller owns it and must free it with [destroyDocument]
     * @throws IllegalArgumentException if [entropy] is not 32 bytes
     */
    suspend fun putToPlatform(
        documentHandle: Pointer,
        dataContractId: String,
        documentTypeName: String,
        entropy: ByteArray,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        require(entropy.size == 32) { "entropy must be 32 bytes, was ${entropy.size}" }
        val entropyMem = Memory(32).apply { write(0, entropy, 0, 32) }
        val ps = settings?.toNative()
        val confirmed = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_document_put_to_platform_and_wait(
                sdkHandle, documentHandle, dataContractId, documentTypeName, entropyMem,
                signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(entropyMem)
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        confirmed
    }

    /**
     * Replace a document on platform and wait for confirmation. Network call — requires a live node.
     *
     * @param documentHandle the updated document to broadcast
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the confirmed DocumentHandle; the caller owns it and must free it with [destroyDocument]
     */
    suspend fun replaceOnPlatform(
        documentHandle: Pointer,
        dataContractId: String,
        documentTypeName: String,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        val ps = settings?.toNative()
        val confirmed = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_document_replace_on_platform_and_wait(
                sdkHandle, documentHandle, dataContractId, documentTypeName,
                signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        confirmed
    }

    /**
     * Delete a document and wait for confirmation. Network call — requires a live node.
     *
     * @param documentId document ID (base58)
     * @param ownerId owner identity ID (base58)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the FFI result data [Pointer] (opaque; on the delete path the caller does not own a handle)
     */
    suspend fun deleteDocument(
        documentId: String,
        ownerId: String,
        dataContractId: String,
        documentTypeName: String,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        val ps = settings?.toNative()
        val result = ResultUnwrapper.unwrap(
            ffi.dash_sdk_document_delete_and_wait(
                sdkHandle, documentId, ownerId, dataContractId, documentTypeName,
                signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        result
    }

    /**
     * Transfer a document to another identity and wait for confirmation. Network call.
     *
     * @param documentHandle the document to transfer
     * @param recipientId recipient identity ID (base58)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the confirmed DocumentHandle; the caller owns it and must free it with [destroyDocument]
     */
    suspend fun transferToIdentity(
        documentHandle: Pointer,
        recipientId: String,
        dataContractId: String,
        documentTypeName: String,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        val ps = settings?.toNative()
        val confirmed = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_document_transfer_to_identity_and_wait(
                sdkHandle, documentHandle, recipientId, dataContractId, documentTypeName,
                signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        confirmed
    }

    /**
     * Purchase a document at [price] and wait for confirmation. Network call.
     *
     * @param documentHandle the document to purchase
     * @param price purchase price in credits
     * @param purchaserId purchaser identity ID (base58)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the confirmed DocumentHandle; the caller owns it and must free it with [destroyDocument]
     */
    suspend fun purchase(
        documentHandle: Pointer,
        dataContractId: String,
        documentTypeName: String,
        price: ULong,
        purchaserId: String,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        val ps = settings?.toNative()
        val confirmed = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_document_purchase_and_wait(
                sdkHandle, documentHandle, dataContractId, documentTypeName, price.toLong(),
                purchaserId, signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        confirmed
    }

    /**
     * Update a document's listed price and wait for confirmation. Network call.
     *
     * @param documentHandle the document whose price changes
     * @param price new price in credits
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; keep alive across the call
     * @return the confirmed DocumentHandle; the caller owns it and must free it with [destroyDocument]
     */
    suspend fun updatePrice(
        documentHandle: Pointer,
        dataContractId: String,
        documentTypeName: String,
        price: ULong,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings? = null
    ): Pointer = withContext(Dispatchers.IO) {
        val ps = settings?.toNative()
        val confirmed = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_document_update_price_of_document_and_wait(
                sdkHandle, documentHandle, dataContractId, documentTypeName, price.toLong(),
                signingKeyHandle, signer.handle, null, ps?.pointer, null
            )
        )
        Reference.reachabilityFence(signer)
        Reference.reachabilityFence(ps)
        confirmed
    }

    /**
     * Free a DocumentHandle produced by [createDocument], [makeHandle], or any `_and_wait`
     * write result. Uses `dash_sdk_document_free` (the header pairs both
     * `dash_sdk_document_free` and `dash_sdk_document_handle_destroy` with these handles —
     * both are `void(DocumentHandle*)`; this service standardises on the former).
     */
    fun destroyDocument(documentHandle: Pointer) =
        ffi.dash_sdk_document_free(documentHandle)

    /**
     * Read a [DashSDKDocumentInfo][DashSDKDocumentInfoNative] off a DocumentHandle.
     * Reads the heap struct returned by `dash_sdk_document_get_info`, copies the fields
     * out, and frees it. The struct exposes no JSON-properties blob (document data lives in
     * the opaque `data_fields` array, not modeled on the read path), so [propertiesJson]
     * is left as the empty object "{}".
     */
    private fun parseDocumentHandle(handle: Pointer): Document {
        val infoPtr = ffi.dash_sdk_document_get_info(handle)
            ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "document_get_info returned null")
        try {
            val info = DashSDKDocumentInfoNative(infoPtr).apply { read() }
            return Document(
                id = info.id.orEmpty(),
                ownerId = info.owner_id.orEmpty(),
                dataContractId = info.data_contract_id.orEmpty(),
                documentType = info.document_type.orEmpty(),
                revision = info.revision,
                createdAt = info.created_at,
                updatedAt = info.updated_at,
                propertiesJson = "{}"
            )
        } finally {
            ffi.dash_sdk_document_info_free(infoPtr)
        }
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
