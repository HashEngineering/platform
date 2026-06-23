package org.dash.sdk.ffi

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure
import com.sun.jna.ptr.IntByReference
import com.sun.jna.ptr.LongByReference

/**
 * JNA interface mapping directly to the C functions in librs_sdk_ffi.so.
 *
 * This mirrors how Swift imports the C header via `import DashSDKFFI`.
 * Function signatures match dash_sdk_ffi.h exactly.
 *
 * Usage:
 *   val sdk = DashSdkFfi.INSTANCE
 */
interface DashSdkFfi : Library {

    // -------------------------------------------------------------------------
    // Global / process-wide (no SDK handle)
    // -------------------------------------------------------------------------

    /** Initialize global SDK state (logging/runtime). Idempotent; call once at startup. */
    fun dash_sdk_init()

    /** SDK version string (static storage — do not free). Header: `const char *`. */
    fun dash_sdk_version(): String?

    /** Set the global log verbosity (0 = off; higher = more verbose). */
    fun dash_sdk_enable_logging(level: Byte)

    // -------------------------------------------------------------------------
    // SDK lifecycle
    // -------------------------------------------------------------------------

    /** Create a new SDK instance. Returns [DashSDKResultNative] by value (struct). */
    fun dash_sdk_create(config: DashSDKConfigNative): DashSDKResultNative

    /**
     * Create a new SDK instance using trusted context providers (fetches quorum info
     * from Dash Core Group-operated nodes). Suitable for testnet/mainnet without
     * manually supplying DAPI addresses — pass [DashSDKConfigNative.dapi_addresses] = null
     * to use built-in defaults for the selected network.
     */
    fun dash_sdk_create_trusted(config: DashSDKConfigNative): DashSDKResultNative

    /** Destroy an SDK instance created by [dash_sdk_create] or [dash_sdk_create_trusted]. */
    fun dash_sdk_destroy(handle: Pointer)

    /** Return the last error message for the current SDK instance (caller must free). */
    fun dash_sdk_get_last_error(handle: Pointer): String?

    /** Free a C string returned by the SDK (Rust: dash_sdk_string_free). */
    fun dash_sdk_string_free(s: Pointer)

    /** Free a heap-allocated [DashSDKError] struct returned inside a result. */
    fun dash_sdk_error_free(error: Pointer)

    // -------------------------------------------------------------------------
    // Identity queries
    // -------------------------------------------------------------------------

    /**
     * Fetch an identity by its hex-encoded ID.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.ResultIdentityHandle].
     * Caller must free the result with [dash_sdk_result_free].
     */
    fun dash_sdk_identity_fetch(handle: Pointer, identity_id_hex: String): DashSDKResultNative

    /**
     * Fetch an identity's balance by its hex-encoded ID.
     * Returns a [DashSDKResult] with the balance in credits as a uint64.
     */
    fun dash_sdk_identity_fetch_balance(handle: Pointer, identity_id_hex: String): DashSDKResultNative

    /**
     * Get identity info from an IdentityHandle. Header:
     * `struct DashSDKIdentityInfo *dash_sdk_identity_get_info(const IdentityHandle *)`.
     * Returns a heap pointer to a [DashSDKIdentityInfoNative] (or null); read it, then
     * release it with [dash_sdk_identity_info_free]. (NOT a DashSDKResult.)
     */
    fun dash_sdk_identity_get_info(identity_handle: Pointer): Pointer?

    /** Free a [DashSDKIdentityInfoNative] returned by [dash_sdk_identity_get_info]. */
    fun dash_sdk_identity_info_free(info: Pointer)

    // --- Identity read-path batch (Phase 1a, batch 2) ---

    /**
     * Fetch an identity's balance and revision by its hex-encoded ID.
     * Returns a [DashSDKResult] with a JSON string (balance + revision).
     */
    fun dash_sdk_identity_fetch_balance_and_revision(handle: Pointer, identity_id: String): DashSDKResultNative

    /**
     * Fetch an identity by a (unique) public key hash.
     * Returns a [DashSDKResult] with a JSON string of the identity, or null if not found.
     */
    fun dash_sdk_identity_fetch_by_public_key_hash(handle: Pointer, public_key_hash: String): DashSDKResultNative

    /**
     * Fetch identities by a non-unique public key hash, paginated by [start_after] (may be null).
     * Returns a [DashSDKResult] with a JSON string.
     */
    fun dash_sdk_identity_fetch_by_non_unique_public_key_hash(
        handle: Pointer,
        public_key_hash: String,
        start_after: String?
    ): DashSDKResultNative

    /**
     * Fetch the identity contract nonce.
     * Returns a [DashSDKResult] with a C string.
     */
    fun dash_sdk_identity_fetch_contract_nonce(
        handle: Pointer,
        identity_id: String,
        contract_id: String
    ): DashSDKResultNative

    /**
     * Fetch the identity nonce.
     * Returns a [DashSDKResult] with the nonce as a C string.
     */
    fun dash_sdk_identity_fetch_nonce(handle: Pointer, identity_id: String): DashSDKResultNative

    /**
     * Fetch an identity by its hex-encoded ID, returning a handle.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.IDENTITY_HANDLE];
     * caller must free the handle with [dash_sdk_identity_destroy].
     */
    fun dash_sdk_identity_fetch_handle(handle: Pointer, identity_id: String): DashSDKResultNative

    /**
     * Fetch an identity's public keys.
     * Returns a [DashSDKResult] with a JSON string of the public keys.
     */
    fun dash_sdk_identity_fetch_public_keys(handle: Pointer, identity_id: String): DashSDKResultNative

    /**
     * Fetch token balances for a single identity over a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → balances).
     */
    fun dash_sdk_identity_fetch_token_balances(
        handle: Pointer,
        identity_id: String,
        token_ids: String
    ): DashSDKResultNative

    /**
     * Fetch token information for a single identity over a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → info).
     */
    fun dash_sdk_identity_fetch_token_infos(
        handle: Pointer,
        identity_id: String,
        token_ids: String
    ): DashSDKResultNative

    /**
     * Fetch contract keys for multiple identities.
     * [identity_ids] is comma-separated or a JSON array; [purposes] is the purpose list.
     * [document_type_name] may be null.
     * Returns a [DashSDKResult] with a JSON string (identity IDs → contract keys by purpose).
     */
    fun dash_sdk_identities_fetch_contract_keys(
        handle: Pointer,
        identity_ids: String,
        contract_id: String,
        document_type_name: String?,
        purposes: String
    ): DashSDKResultNative

    /**
     * Fetch token balances for multiple identities for a single token.
     * Returns a [DashSDKResult] with a JSON string (identity IDs → balances).
     */
    fun dash_sdk_identities_fetch_token_balances(
        handle: Pointer,
        identity_ids: String,
        token_id: String
    ): DashSDKResultNative

    /**
     * Fetch token information for multiple identities for a single token.
     * Returns a [DashSDKResult] with a JSON string (identity IDs → info).
     */
    fun dash_sdk_identities_fetch_token_infos(
        handle: Pointer,
        identity_ids: String,
        token_id: String
    ): DashSDKResultNative

    /**
     * Resolve a name to an identity.
     * Returns a [DashSDKResult] with a JSON string.
     */
    fun dash_sdk_identity_resolve_name(handle: Pointer, name: String): DashSDKResultNative

    /**
     * Parse an identity from a JSON string into an identity handle (process-local; no network).
     * Returns a [DashSDKResult] with [DashSDKResultDataType.IDENTITY_HANDLE];
     * caller must free the handle with [dash_sdk_identity_destroy].
     */
    fun dash_sdk_identity_parse_json(json_str: String): DashSDKResultNative

    /**
     * Get a public key from an identity handle by its key ID.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.PUBLIC_KEY_HANDLE];
     * caller must free the handle with [dash_sdk_identity_public_key_destroy].
     */
    fun dash_sdk_identity_get_public_key_by_id(identity: Pointer, key_id: Byte): DashSDKResultNative

    /**
     * Get the appropriate signing key for a state transition.
     * [transition_type] mirrors the C `StateTransitionType` enum — see [StateTransitionType].
     * Returns a [DashSDKResult] with [DashSDKResultDataType.PUBLIC_KEY_HANDLE];
     * caller must free the handle with [dash_sdk_identity_public_key_destroy].
     */
    fun dash_sdk_identity_get_signing_key_for_transition(
        identity_handle: Pointer,
        transition_type: Int
    ): DashSDKResultNative

    /** Get the key ID (uint32) from an IdentityPublicKeyHandle. Returns 0 if the handle is null. */
    fun dash_sdk_identity_public_key_get_id(key_handle: Pointer): Int

    /** Free an IdentityPublicKeyHandle. */
    fun dash_sdk_identity_public_key_destroy(handle: Pointer)

    /**
     * Destroy an IdentityHandle (the header-true free for handles from `*_fetch_handle`,
     * `*_parse_json`, `*_fetch`, etc.).
     */
    fun dash_sdk_identity_destroy(handle: Pointer)

    // -------------------------------------------------------------------------
    // Data contract queries
    // -------------------------------------------------------------------------

    /**
     * Fetch a data contract by its hex-encoded ID.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.ResultDataContractHandle].
     */
    fun dash_sdk_data_contract_fetch(handle: Pointer, contract_id_hex: String): DashSDKResultNative

    /**
     * Fetch a data contract as a JSON string.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.String].
     */
    fun dash_sdk_data_contract_fetch_json(handle: Pointer, contract_id_hex: String): DashSDKResultNative

    /**
     * Fetch a data contract's revision history. Returns a [DashSDKResult] with a JSON string.
     * @param start_at_ms only history entries at/after this timestamp (ms); 0 = from genesis.
     */
    fun dash_sdk_data_contract_fetch_history(
        handle: Pointer,
        contract_id: String,
        limit: Int,
        offset: Int,
        start_at_ms: Long
    ): DashSDKResultNative

    /**
     * Get the JSON schema for [document_type] from a DataContractHandle. Header:
     * `char *dash_sdk_data_contract_get_schema(const DataContractHandle *, const char *)`.
     * Returns a heap C string (or null) — read it, then free with [dash_sdk_string_free].
     */
    fun dash_sdk_data_contract_get_schema(contract_handle: Pointer, document_type: String): Pointer?

    /** Free a DataContractHandle (header-true destroy). */
    fun dash_sdk_data_contract_destroy(contract_handle: Pointer)

    /**
     * Fetch a data contract by ID, optionally returning a JSON string and/or serialized
     * bytes alongside the handle. Header:
     * `struct DashSDKDataContractFetchResult dash_sdk_data_contract_fetch_with_serialization(
     *   const SDKHandle *, const char *contract_id, bool return_json, bool return_serialized)`.
     * Returns the result struct **by value** ([DashSDKDataContractFetchResultNative]); free its
     * inner heap pointers with [dash_sdk_data_contract_fetch_result_free].
     */
    fun dash_sdk_data_contract_fetch_with_serialization(
        sdk_handle: Pointer,
        contract_id: String,
        return_json: Boolean,
        return_serialized: Boolean
    ): DashSDKDataContractFetchResultNative

    /**
     * Free the inner heap-allocated buffers of a [DashSDKDataContractFetchResultNative].
     * The outer struct lives on the caller's stack (returned by value) and is NOT freed —
     * pass the backing pointer of the returned struct. After the call every inner pointer
     * is nulled, so a redundant call is a no-op.
     */
    fun dash_sdk_data_contract_fetch_result_free(result: Pointer)

    // -------------------------------------------------------------------------
    // Document queries
    // -------------------------------------------------------------------------

    /**
     * Fetch a document by its hex-encoded ID.
     * Returns a [DashSDKResult] with [DashSDKResultDataType.ResultDocumentHandle].
     */
    fun dash_sdk_document_fetch(
        handle: Pointer,
        contract_handle: Pointer,
        document_type: String,
        document_id_hex: String
    ): DashSDKResultNative

    /**
     * Search documents with optional where/order-by JSON filters.
     * Returns a [DashSDKResult] with a JSON array string of documents.
     */
    fun dash_sdk_document_search(
        handle: Pointer,
        params: DashSDKDocumentSearchParamsNative
    ): DashSDKResultNative

    /**
     * Destroy a DocumentHandle from a fetch/query. Header:
     * `struct DashSDKError *dash_sdk_document_destroy(SDKHandle *, DocumentHandle *)`.
     * Takes the SDK handle (matches the Swift query-path cleanup). Returns a
     * `DashSDKError *` (null on success); free a non-null error with [dash_sdk_error_free].
     */
    fun dash_sdk_document_destroy(sdk_handle: Pointer, document_handle: Pointer): Pointer?

    /**
     * Get document info from a DocumentHandle. Header:
     * `struct DashSDKDocumentInfo *dash_sdk_document_get_info(const DocumentHandle *)`.
     * Returns a heap pointer to a [DashSDKDocumentInfoNative] (or null); read it, then
     * release it with [dash_sdk_document_info_free]. (NOT a DashSDKResult.)
     */
    fun dash_sdk_document_get_info(document_handle: Pointer): Pointer?

    /** Free a [DashSDKDocumentInfoNative] returned by [dash_sdk_document_get_info]. */
    fun dash_sdk_document_info_free(info: Pointer)

    // -------------------------------------------------------------------------
    // DPNS queries
    // -------------------------------------------------------------------------

    /** Resolve a DPNS name to an identity ID. Returns JSON string result. */
    fun dash_sdk_dpns_resolve(handle: Pointer, name: String): DashSDKResultNative

    /** Check if a DPNS name is available. Returns bool result. */
    fun dash_sdk_dpns_check_availability(handle: Pointer, name: String): DashSDKResultNative

    /** Search DPNS names by prefix. Returns JSON array result. */
    fun dash_sdk_dpns_search(handle: Pointer, prefix: String, limit: Int): DashSDKResultNative

    // -------------------------------------------------------------------------
    // Token queries (read-path; return DashSDKResult carrying a JSON / base58 string)
    // -------------------------------------------------------------------------

    /**
     * Derive the canonical platform token ID for `(contract_id, position)`.
     * Pure helper — no SDK handle, no network. [position] is a C `uint16_t`
     * (unsigned 16-bit), passed in an Int.
     * Returns a [DashSDKResult] with a base58-encoded token-ID string.
     */
    fun dash_sdk_calculate_token_id(contract_id: String, position: Int): DashSDKResultNative

    /**
     * Get a token's contract info (contract ID and token position).
     * Returns a [DashSDKResult] with a JSON string (or null if not found).
     */
    fun dash_sdk_token_get_contract_info(handle: Pointer, token_id: String): DashSDKResultNative

    /**
     * Get direct purchase prices for a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → pricing info).
     */
    fun dash_sdk_token_get_direct_purchase_prices(handle: Pointer, token_ids: String): DashSDKResultNative

    /**
     * Get token balances for a single identity over a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → balances).
     */
    fun dash_sdk_token_get_identity_balances(
        handle: Pointer,
        identity_id: String,
        token_ids: String
    ): DashSDKResultNative

    /**
     * Get token information for a single identity over a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → info).
     */
    fun dash_sdk_token_get_identity_infos(
        handle: Pointer,
        identity_id: String,
        token_ids: String
    ): DashSDKResultNative

    /**
     * Get the last perpetual-distribution claim for [identity_id] on [token_id].
     * Returns a [DashSDKResult] with a JSON string.
     */
    fun dash_sdk_token_get_perpetual_distribution_last_claim(
        handle: Pointer,
        token_id: String,
        identity_id: String
    ): DashSDKResultNative

    /**
     * Get pre-programmed distributions for a token.
     * [start_time_ms] is a C `uint64_t` (0 = no start time); [start_recipient] may be null;
     * [limit] is a C `uint32_t` (unsigned 32-bit; 0 = default limit).
     * Returns a [DashSDKResult] with a JSON array string (or null if not found).
     */
    fun dash_sdk_token_get_pre_programmed_distributions(
        handle: Pointer,
        token_id: String,
        start_time_ms: Long,
        start_recipient: String?,
        start_recipient_included: Boolean,
        limit: Int
    ): DashSDKResultNative

    /**
     * Get statuses for a comma-separated list of token IDs.
     * Returns a [DashSDKResult] with a JSON string (token IDs → status info).
     */
    fun dash_sdk_token_get_statuses(handle: Pointer, token_ids: String): DashSDKResultNative

    /**
     * Get the total supply of a token.
     * Returns a [DashSDKResult] with a JSON string (or null if not found).
     */
    fun dash_sdk_token_get_total_supply(handle: Pointer, token_id: String): DashSDKResultNative

    // -------------------------------------------------------------------------
    // System / status / protocol-version queries (read-path; return DashSDKResult)
    // -------------------------------------------------------------------------

    /** Network the handle was created for (FFINetwork enum value: 0=Mainnet,1=Testnet,…). */
    fun dash_sdk_get_network(handle: Pointer): Int

    /** Overall SDK status. Returns a [DashSDKResultNative] with a JSON string. */
    fun dash_sdk_get_status(handle: Pointer): DashSDKResultNative

    /** Platform-only status. Returns a [DashSDKResultNative] with a JSON string. */
    fun dash_sdk_get_platform_status(handle: Pointer): DashSDKResultNative

    /**
     * Epoch info for [count] epochs starting at [start_epoch] (null = current),
     * ordered by [ascending]. Returns a JSON string result.
     */
    fun dash_sdk_system_get_epochs_info(
        handle: Pointer,
        start_epoch: String?,
        count: Int,
        ascending: Boolean
    ): DashSDKResultNative

    /** Current quorums info. Returns a JSON string result. */
    fun dash_sdk_system_get_current_quorums_info(handle: Pointer): DashSDKResultNative

    /**
     * Total credits currently in platform. Returns a uint64 result.
     * Not yet surfaced via [org.dash.sdk.services.SystemService]: the numeric
     * `DashSDKResult` representation must be confirmed before adding a typed unwrap.
     */
    fun dash_sdk_system_get_total_credits_in_platform(handle: Pointer): DashSDKResultNative

    /**
     * Prefunded specialized balance for the hex-encoded [id]. Returns a uint64 result.
     * Bindings-only for now (see [dash_sdk_system_get_total_credits_in_platform]).
     */
    fun dash_sdk_system_get_prefunded_specialized_balance(handle: Pointer, id: String): DashSDKResultNative

    /** Protocol-version upgrade state. Returns a JSON string result. */
    fun dash_sdk_protocol_version_get_upgrade_state(handle: Pointer): DashSDKResultNative

    /**
     * Protocol-version upgrade vote status from [start_pro_tx_hash] over [count]
     * masternodes. Returns a JSON string result.
     */
    fun dash_sdk_protocol_version_get_upgrade_vote_status(
        handle: Pointer,
        start_pro_tx_hash: String,
        count: Int
    ): DashSDKResultNative

    // Note: the library exposes no error-accessor functions. A DashSDKError is read by
    // its struct fields directly — `code` (C int @0) and `message` (char* @POINTER_SIZE) —
    // see ResultUnwrapper / DashSDK.create. Only dash_sdk_error_free is exported.

    companion object {
        val INSTANCE: DashSdkFfi by lazy {
            NativeLoader.load()
            Native.load(NativeLibrary.name, DashSdkFfi::class.java)
        }
    }
}

// ---------------------------------------------------------------------------
// JNA Structure mappings for C structs in dash_sdk_ffi.h
// ---------------------------------------------------------------------------

/**
 * Maps to `struct DashSDKIdentityInfo` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_identity_get_info].
 *
 * 64-bit layout (all naturally aligned):
 *   id: char* (8) + balance: u64 (8) + revision: u64 (8) + public_keys_count: u32 (4)
 *   + 4 tail padding = 32 bytes.
 *
 * Usage: construct over the returned pointer, [read], copy the fields out, then release
 * the pointer with [DashSdkFfi.dash_sdk_identity_info_free] (which frees the heap `id`
 * string too) — do not touch [id] after freeing.
 */
@Structure.FieldOrder("id", "balance", "revision", "public_keys_count")
class DashSDKIdentityInfoNative : Structure {
    /** Identity ID as a hex string (heap `char*`). */
    @JvmField var id: String? = null
    /** Balance in credits. */
    @JvmField var balance: Long = 0
    /** Revision number. */
    @JvmField var revision: Long = 0
    /** Number of public keys on the identity. */
    @JvmField var public_keys_count: Int = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKDocumentInfo` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_document_get_info].
 *
 * C 64-bit layout (all naturally 8-aligned; no padding):
 *   id:                 char*     @0   (8)
 *   owner_id:           char*     @8   (8)
 *   data_contract_id:   char*     @16  (8)
 *   document_type:      char*     @24  (8)
 *   revision:           u64       @32  (8)
 *   created_at:         i64       @40  (8)
 *   updated_at:         i64       @48  (8)
 *   data_fields_count:  uintptr_t @56  (8)
 *   data_fields:        DashSDKDocumentField* @64 (8)
 *   = 72 bytes
 *
 * Usage: construct over the returned pointer, [read], copy the fields out, then release
 * the pointer with [DashSdkFfi.dash_sdk_document_info_free] (which frees the heap strings
 * and the data_fields array) — do not touch the string/pointer fields after freeing.
 *
 * [data_fields] is left as an opaque [Pointer] (the field-array is not read on the
 * read path); modeling DashSDKDocumentField would be required before dereferencing it.
 */
@Structure.FieldOrder(
    "id",
    "owner_id",
    "data_contract_id",
    "document_type",
    "revision",
    "created_at",
    "updated_at",
    "data_fields_count",
    "data_fields"
)
class DashSDKDocumentInfoNative : Structure {
    /** Document ID as a hex string (heap `char*`). */
    @JvmField var id: String? = null
    /** Owner ID as a hex string (heap `char*`). */
    @JvmField var owner_id: String? = null
    /** Data contract ID as a hex string (heap `char*`). */
    @JvmField var data_contract_id: String? = null
    /** Document type (heap `char*`). */
    @JvmField var document_type: String? = null
    /** Revision number. */
    @JvmField var revision: Long = 0
    /** Created-at timestamp (ms since epoch). */
    @JvmField var created_at: Long = 0
    /** Updated-at timestamp (ms since epoch). */
    @JvmField var updated_at: Long = 0
    /** Number of data fields in [data_fields]. */
    @JvmField var data_fields_count: Long = 0
    /** Pointer to the `DashSDKDocumentField[]` array (opaque; not read here). */
    @JvmField var data_fields: Pointer? = null

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKDataContractFetchResult` in rs-sdk-ffi.h — returned **by value**
 * from [DashSdkFfi.dash_sdk_data_contract_fetch_with_serialization].
 *
 * C 64-bit layout (all naturally 8-aligned; no padding):
 *   contract_handle:    DataContractHandle* @0  (8)
 *   json_string:        char*               @8  (8)
 *   serialized_data:    uint8_t*            @16 (8)
 *   serialized_data_len:uintptr_t           @24 (8)
 *   error:              DashSDKError*       @32 (8)
 *   = 40 bytes
 *
 * After reading fields out, free the inner heap pointers with
 * [DashSdkFfi.dash_sdk_data_contract_fetch_result_free] (pass this struct's backing
 * pointer). [json_string] is left opaque ([Pointer]) so it is read explicitly before free
 * rather than auto-marshalled twice.
 */
@Structure.FieldOrder(
    "contract_handle",
    "json_string",
    "serialized_data",
    "serialized_data_len",
    "error"
)
class DashSDKDataContractFetchResultNative : Structure(), Structure.ByValue {
    /** Handle to the fetched data contract (null on error or if not requested). */
    @JvmField var contract_handle: Pointer? = null
    /** JSON representation of the contract (heap `char*`; null on error or if not requested). */
    @JvmField var json_string: Pointer? = null
    /** Serialized contract bytes (heap `uint8_t*`; null on error or if not requested). */
    @JvmField var serialized_data: Pointer? = null
    /** Length of [serialized_data] in bytes. */
    @JvmField var serialized_data_len: Long = 0
    /** Heap-allocated [DashSDKError] (null on success). */
    @JvmField var error: Pointer? = null
}

/**
 * Maps to DashSDKConfig in dash_sdk_ffi.h.
 *
 * Field order and types must match the Rust repr(C) struct exactly:
 *   network: u32 (4) + padding (4) + dapi_addresses: *const c_char (8)
 *   + skip_asset_lock_proof_verification: bool (1) + padding (3)
 *   + request_retry_count: u32 (4) + request_timeout_ms: u64 (8)
 *   + quorum_url: *const c_char (8) + platform_version: u32 (4) + padding (4)
 *   = 48 bytes
 */
@Structure.FieldOrder(
    "network",
    "dapi_addresses",
    "skip_asset_lock_proof_verification",
    "request_retry_count",
    "request_timeout_ms",
    "quorum_url",
    "platform_version"
)
class DashSDKConfigNative : Structure() {
    /** DashSDKNetwork enum value (0=Mainnet, 1=Testnet, 2=Regtest, 3=Devnet, 4=Local) */
    @JvmField var network: Int = 1
    /** Comma-separated DAPI addresses, e.g. "http://127.0.0.1:3000"; null = mock SDK */
    @JvmField var dapi_addresses: String? = null
    /** bool (1 byte): use Byte, not Boolean — JNA maps Boolean to 4 bytes */
    @JvmField var skip_asset_lock_proof_verification: Byte = 0
    @JvmField var request_retry_count: Int = 3
    @JvmField var request_timeout_ms: Long = 30_000L
    /**
     * Optional trusted-context-provider quorum lookup base URL; null = derive
     * the default endpoint from [network]. Only honored on the
     * `dash_sdk_create_trusted` path. Must be present in the struct regardless:
     * Rust reads this field by offset, so omitting it leaves the pointer
     * reading past the allocation and crashes in strlen.
     */
    @JvmField var quorum_url: String? = null
    /** Pin to a specific protocol version; 0 = SDK default (auto-detect). */
    @JvmField var platform_version: Int = 0
}

/**
 * Maps to DashSDKDocumentSearchParams in dash_sdk_ffi.h.
 */
@Structure.FieldOrder(
    "data_contract_handle",
    "document_type",
    "where_json",
    "order_by_json",
    "limit",
    "start_at"
)
class DashSDKDocumentSearchParamsNative : Structure() {
    @JvmField var data_contract_handle: Pointer? = null
    @JvmField var document_type: String? = null
    @JvmField var where_json: String? = null
    @JvmField var order_by_json: String? = null
    @JvmField var limit: Int = 0
    @JvmField var start_at: Int = 0
}

// ---------------------------------------------------------------------------
// DashSDKResult — returned by value from dash_sdk_create
// ---------------------------------------------------------------------------

/**
 * Maps to DashSDKResult in dash_sdk_ffi.h (returned by value, not pointer).
 *
 * Rust layout (64-bit):
 *   data_type: u32 (4) + padding (4) + data: *mut c_void (8) + error: *mut DashSDKError (8) = 24 bytes
 *
 * On success:  error == null, data holds the opaque SDK handle pointer.
 * On failure:  data == null, error holds a heap-allocated DashSDKError pointer.
 */
@Structure.FieldOrder("data_type", "data", "error")
class DashSDKResultNative : Structure(), Structure.ByValue {
    @JvmField var data_type: Int = 0
    @JvmField var data: Pointer? = null
    @JvmField var error: Pointer? = null
}

// ---------------------------------------------------------------------------
// Error code constants (mirrors DashSDKErrorCode enum)
// ---------------------------------------------------------------------------
object DashSDKErrorCode {
    const val SUCCESS = 0
    const val INVALID_PARAMETER = 1
    const val INVALID_STATE = 2
    const val NETWORK_ERROR = 3
    const val SERIALIZATION_ERROR = 4
    const val PROTOCOL_ERROR = 5
    const val CRYPTO_ERROR = 6
    const val NOT_FOUND = 7
    const val TIMEOUT = 8
    const val NOT_IMPLEMENTED = 9
    const val INTERNAL_ERROR = 99
}

// Result data type constants (mirrors DashSDKResultDataType enum)
object DashSDKResultDataType {
    const val NONE = 0
    const val STRING = 1
    const val BINARY_DATA = 2
    const val IDENTITY_HANDLE = 3
    const val DOCUMENT_HANDLE = 4
    const val DATA_CONTRACT_HANDLE = 5
    const val IDENTITY_BALANCE_MAP = 6
    const val PUBLIC_KEY_HANDLE = 7
}

// State transition type constants (mirrors the C `StateTransitionType` enum;
// passed as a C int to dash_sdk_identity_get_signing_key_for_transition).
object StateTransitionType {
    const val IDENTITY_UPDATE = 0
    const val IDENTITY_TOP_UP = 1
    const val IDENTITY_CREDIT_TRANSFER = 2
    const val IDENTITY_CREDIT_WITHDRAWAL = 3
    const val DOCUMENTS_BATCH = 4
    const val DATA_CONTRACT_CREATE = 5
    const val DATA_CONTRACT_UPDATE = 6
}

// Network constants (mirrors DashSDKNetwork enum)
object DashSDKNetwork {
    const val MAINNET = 0
    const val TESTNET = 1
    const val REGTEST = 2
    const val DEVNET = 3
    const val LOCAL = 4
}
