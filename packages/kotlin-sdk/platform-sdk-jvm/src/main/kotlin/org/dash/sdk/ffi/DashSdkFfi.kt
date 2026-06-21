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

    /** Get identity info from an IdentityHandle. */
    fun dash_sdk_identity_get_info(identity_handle: Pointer): DashSDKResultNative

    /** Free an IdentityHandle. */
    fun dash_sdk_identity_handle_free(identity_handle: Pointer)

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

    /** Free a DataContractHandle. */
    fun dash_sdk_data_contract_handle_free(contract_handle: Pointer)

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

    /** Free a DocumentHandle. */
    fun dash_sdk_document_handle_free(document_handle: Pointer)

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

    // -------------------------------------------------------------------------
    // Helper: result accessors
    // -------------------------------------------------------------------------

    /** Get error code from a [DashSDKError] pointer (read field at offset 0). */
    fun dash_sdk_error_get_code(error: Pointer): Int

    /** Get error message from a [DashSDKError] pointer (do not free separately). */
    fun dash_sdk_error_get_message(error: Pointer): String?

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

// Network constants (mirrors DashSDKNetwork enum)
object DashSDKNetwork {
    const val MAINNET = 0
    const val TESTNET = 1
    const val REGTEST = 2
    const val DEVNET = 3
    const val LOCAL = 4
}
