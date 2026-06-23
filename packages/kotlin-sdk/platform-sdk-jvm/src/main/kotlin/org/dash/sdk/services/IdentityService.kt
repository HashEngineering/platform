package org.dash.sdk.services

import com.sun.jna.Memory
import com.sun.jna.NativeLong
import com.sun.jna.Pointer
import java.lang.ref.Reference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSDKErrorCode
import org.dash.sdk.ffi.DashSDKIdentityInfoNative
import org.dash.sdk.ffi.DashSDKPublicKeyDataNative
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Identity
import org.dash.sdk.models.IdentityPublicKeyParams

/**
 * High-level service for Dash Platform identity operations.
 * Mirrors SwiftDashSDK/Identity.swift.
 */
class IdentityService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Fetch an identity by its hex-encoded ID.
     *
     * @param identityId 32-byte identity ID as a hex string
     * @return [Identity] on success
     * @throws org.dash.sdk.models.DashSDKException on failure
     */
    suspend fun fetchIdentity(identityId: String): Identity = withContext(Dispatchers.IO) {
        // dash_sdk_identity_fetch returns a JSON *string*; only dash_sdk_identity_fetch_handle
        // returns a real IdentityHandle that get_info/destroy can operate on. Passing the
        // JSON-string pointer to identity_destroy crashes (it reads the base58 id text as a
        // BTreeMap). See rs-sdk-ffi.h notes on dash_sdk_identity_fetch / _fetch_handle.
        val result = ffi.dash_sdk_identity_fetch_handle(sdkHandle, identityId)
        val identityHandle = ResultUnwrapper.unwrapHandle(result)
        try {
            parseIdentityInfo(identityHandle)
        } finally {
            ffi.dash_sdk_identity_destroy(identityHandle)
        }
    }

    /**
     * Fetch only the balance for an identity.
     *
     * @param identityId 32-byte identity ID as a hex string
     * @return balance in credits
     */
    suspend fun fetchBalance(identityId: String): Long = withContext(Dispatchers.IO) {
        val result = ffi.dash_sdk_identity_fetch_balance(sdkHandle, identityId)
        val dataPtr = ResultUnwrapper.unwrap(result)
        // The result data is a uint64_t pointer
        dataPtr.getLong(0)
    }

    // -------------------------------------------------------------------------
    // Identity read-path batch (Phase 1a, batch 2) — 1:1 FFI wrappers.
    // Each returns the raw FFI payload (JSON string or handle) unwrapped via
    // ResultUnwrapper; no parsing, derivation, or orchestration here.
    // -------------------------------------------------------------------------

    /** Fetch balance and revision as a JSON string. */
    suspend fun fetchBalanceAndRevision(identityId: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_identity_fetch_balance_and_revision(sdkHandle, identityId)
        )
    }

    /** Fetch an identity by a unique public key hash; returns a JSON string. */
    suspend fun fetchByPublicKeyHash(publicKeyHash: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_identity_fetch_by_public_key_hash(sdkHandle, publicKeyHash)
        )
    }

    /** Fetch identities by a non-unique public key hash; returns a JSON string. */
    suspend fun fetchByNonUniquePublicKeyHash(
        publicKeyHash: String,
        startAfter: String? = null
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_identity_fetch_by_non_unique_public_key_hash(sdkHandle, publicKeyHash, startAfter)
        )
    }

    /** Fetch the identity contract nonce as a string. */
    suspend fun fetchContractNonce(identityId: String, contractId: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_identity_fetch_contract_nonce(sdkHandle, identityId, contractId)
            )
        }

    /** Fetch the identity nonce as a string. */
    suspend fun fetchNonce(identityId: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_identity_fetch_nonce(sdkHandle, identityId))
    }

    /**
     * Fetch an identity by ID, returning the raw identity handle.
     * Caller owns the handle and must free it with [destroyIdentity].
     */
    suspend fun fetchHandle(identityId: String): Pointer = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapHandle(ffi.dash_sdk_identity_fetch_handle(sdkHandle, identityId))
    }

    /** Fetch an identity's public keys as a JSON string. */
    suspend fun fetchPublicKeys(identityId: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_identity_fetch_public_keys(sdkHandle, identityId))
    }

    /** Fetch token balances for one identity over a comma-separated token ID list; JSON string. */
    suspend fun fetchTokenBalances(identityId: String, tokenIds: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_identity_fetch_token_balances(sdkHandle, identityId, tokenIds)
            )
        }

    /** Fetch token information for one identity over a comma-separated token ID list; JSON string. */
    suspend fun fetchTokenInfos(identityId: String, tokenIds: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_identity_fetch_token_infos(sdkHandle, identityId, tokenIds)
            )
        }

    /** Fetch contract keys for multiple identities; returns a JSON string. */
    suspend fun fetchContractKeys(
        identityIds: String,
        contractId: String,
        purposes: String,
        documentTypeName: String? = null
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_identities_fetch_contract_keys(
                sdkHandle, identityIds, contractId, documentTypeName, purposes
            )
        )
    }

    /** Fetch token balances for multiple identities for a single token; JSON string. */
    suspend fun fetchTokenBalancesForIdentities(identityIds: String, tokenId: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_identities_fetch_token_balances(sdkHandle, identityIds, tokenId)
            )
        }

    /** Fetch token information for multiple identities for a single token; JSON string. */
    suspend fun fetchTokenInfosForIdentities(identityIds: String, tokenId: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_identities_fetch_token_infos(sdkHandle, identityIds, tokenId)
            )
        }

    /**
     * Fetch balances for multiple identities; returns a JSON string (identity IDs → balances).
     *
     * The FFI takes a contiguous array of 32-byte ID buffers (`const uint8_t (*)[32]`) plus a
     * count. Each [ByteArray] in [identityIds] must be exactly 32 bytes; they are packed into
     * one contiguous native buffer and the COUNT is passed as the length.
     *
     * @param identityIds list of raw 32-byte identity IDs
     * @throws IllegalArgumentException if any ID is not exactly 32 bytes
     */
    suspend fun fetchBalances(identityIds: List<ByteArray>): String = withContext(Dispatchers.IO) {
        require(identityIds.isNotEmpty()) { "identityIds must not be empty" }
        identityIds.forEachIndexed { i, id ->
            require(id.size == 32) { "identity ID at index $i must be 32 bytes, was ${id.size}" }
        }
        // Pack count * 32 bytes contiguously: `const uint8_t (*)[32]` is a pointer to a flat
        // array of fixed 32-byte blocks. Memory is freed when it goes out of scope (GC), which
        // is safe — the FFI only reads it for the duration of the call.
        val buffer = Memory(identityIds.size.toLong() * 32L)
        identityIds.forEachIndexed { i, id ->
            buffer.write(i.toLong() * 32L, id, 0, 32)
        }
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_identities_fetch_balances(sdkHandle, buffer, identityIds.size.toLong())
        )
    }

    /** Resolve a name to an identity; returns a JSON string. */
    suspend fun resolveName(name: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_identity_resolve_name(sdkHandle, name))
    }

    /**
     * Parse an identity from a JSON string into a handle (process-local; no network).
     * Caller owns the handle and must free it with [destroyIdentity].
     */
    suspend fun parseJson(json: String): Pointer = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapHandle(ffi.dash_sdk_identity_parse_json(json))
    }

    /**
     * Build a standalone identity public-key handle from raw components (process-local).
     * Caller owns the handle and must free it with [destroyPublicKey].
     *
     * @param params the key's components (id/type/purpose/security level/data)
     */
    fun createPublicKey(params: IdentityPublicKeyParams): Pointer {
        val dataMem = Memory(params.data.size.toLong())
        dataMem.write(0, params.data, 0, params.data.size)
        return ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_identity_public_key_create_from_data(
                params.keyId,
                params.keyType.toByte(),
                params.purpose.toByte(),
                params.securityLevel.toByte(),
                dataMem,
                NativeLong(params.data.size.toLong()),
                params.readOnly,
                params.disabledAt
            )
        )
    }

    /**
     * Build an identity handle from its components (process-local; no network).
     * Caller owns the handle and must free it with [destroyIdentity].
     *
     * For registration the [identityId] is a placeholder — the IdentityCreate transition
     * derives the real on-chain id from the asset-lock proof. Only the keys matter here.
     *
     * @param identityId 32-byte identity ID (placeholder for registration)
     * @param publicKeys the identity's public keys
     * @param balance starting balance in credits (0 for a new identity)
     * @param revision starting revision (0 for a new identity)
     * @throws IllegalArgumentException if [identityId] is not 32 bytes or [publicKeys] is empty
     */
    suspend fun createFromComponents(
        identityId: ByteArray,
        publicKeys: List<IdentityPublicKeyParams>,
        balance: ULong = 0uL,
        revision: ULong = 0uL,
    ): Pointer = withContext(Dispatchers.IO) {
        require(identityId.size == 32) { "identityId must be 32 bytes, was ${identityId.size}" }
        require(publicKeys.isNotEmpty()) { "publicKeys must not be empty" }

        val idMem = Memory(32)
        idMem.write(0, identityId, 0, 32)

        // Pin each key's data buffer; the struct rows borrow these pointers, so the
        // Memory must stay referenced until the FFI call returns.
        val dataBuffers = publicKeys.map { pk ->
            Memory(pk.data.size.toLong()).apply { write(0, pk.data, 0, pk.data.size) }
        }

        @Suppress("UNCHECKED_CAST")
        val rows = DashSDKPublicKeyDataNative().toArray(publicKeys.size) as Array<DashSDKPublicKeyDataNative>
        publicKeys.forEachIndexed { i, pk ->
            rows[i].apply {
                id = pk.keyId.toByte()
                purpose = pk.purpose.toByte()
                security_level = pk.securityLevel.toByte()
                key_type = pk.keyType.toByte()
                read_only = pk.readOnly
                data = dataBuffers[i]
                data_len = NativeLong(pk.data.size.toLong())
                disabled_at = pk.disabledAt
                write()
            }
        }

        val handle = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_identity_create_from_components(
                idMem,
                rows[0].pointer,
                NativeLong(publicKeys.size.toLong()),
                balance.toLong(),
                revision.toLong()
            )
        )
        // Keep the native buffers reachable until the FFI call has returned — the rows
        // borrow pointers into idMem / dataBuffers, and `rows` owns the contiguous backing.
        Reference.reachabilityFence(idMem)
        Reference.reachabilityFence(dataBuffers)
        Reference.reachabilityFence(rows)
        handle
    }

    /**
     * Get a public key handle from an identity handle by key ID.
     * Caller owns the handle and must free it with [destroyPublicKey].
     */
    suspend fun getPublicKeyById(identityHandle: Pointer, keyId: Byte): Pointer =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapHandle(
                ffi.dash_sdk_identity_get_public_key_by_id(identityHandle, keyId)
            )
        }

    /**
     * Get the appropriate signing key handle for a state transition type
     * (see [org.dash.sdk.ffi.StateTransitionType]).
     * Caller owns the handle and must free it with [destroyPublicKey].
     */
    suspend fun getSigningKeyForTransition(identityHandle: Pointer, transitionType: Int): Pointer =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapHandle(
                ffi.dash_sdk_identity_get_signing_key_for_transition(identityHandle, transitionType)
            )
        }

    /** Get the key ID (uint32) from a public key handle. */
    fun getPublicKeyId(keyHandle: Pointer): Int =
        ffi.dash_sdk_identity_public_key_get_id(keyHandle)

    /** Free a public key handle. */
    fun destroyPublicKey(keyHandle: Pointer) =
        ffi.dash_sdk_identity_public_key_destroy(keyHandle)

    /** Free an identity handle (header-true destroy). */
    fun destroyIdentity(identityHandle: Pointer) =
        ffi.dash_sdk_identity_destroy(identityHandle)

    /**
     * Read a [DashSDKIdentityInfo][DashSDKIdentityInfoNative] off an identity handle.
     * Reads the heap struct returned by `dash_sdk_identity_get_info`, copies the fields
     * out, and frees it — no JSON involved (the previous binding wrongly treated the
     * struct pointer as a JSON-string result).
     */
    /**
     * Read the [Identity] info (id, balance, revision, key count) from an identity handle —
     * e.g. one produced by [createFromComponents], [parseJson], or [fetchHandle].
     * Process-local; does not free the handle.
     */
    fun getInfo(identityHandle: Pointer): Identity = parseIdentityInfo(identityHandle)

    private fun parseIdentityInfo(identityHandle: Pointer): Identity {
        val infoPtr = ffi.dash_sdk_identity_get_info(identityHandle)
            ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "identity_get_info returned null")
        try {
            val info = DashSDKIdentityInfoNative(infoPtr).apply { read() }
            return Identity(
                id = info.id.orEmpty(),
                balance = info.balance,
                revision = info.revision,
                publicKeysCount = info.public_keys_count
            )
        } finally {
            ffi.dash_sdk_identity_info_free(infoPtr)
        }
    }
}
