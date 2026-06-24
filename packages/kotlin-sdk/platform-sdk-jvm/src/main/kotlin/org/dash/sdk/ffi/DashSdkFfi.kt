package org.dash.sdk.ffi

import com.sun.jna.Callback
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.NativeLong
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
     * Fetch balances for multiple identities. Header:
     * `dash_sdk_identities_fetch_balances(const SDKHandle *, const uint8_t (*identity_ids)[32], uintptr_t identity_ids_len)`.
     * [identity_ids] is a pointer to a contiguous array of 32-byte ID buffers (one
     * [com.sun.jna.Memory] of `count * 32` bytes); [identity_ids_len] is the COUNT of IDs
     * (not byte length). Returns a [DashSDKResult] with a JSON string (identity IDs → balances).
     */
    fun dash_sdk_identities_fetch_balances(
        sdk_handle: Pointer,
        identity_ids: Pointer,
        identity_ids_len: Long
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
     * Build a standalone identity public key from raw components (process-local; no network).
     * Returns a [DashSDKResult] with an `IdentityPublicKey` handle; free it with
     * [dash_sdk_identity_public_key_destroy]. [key_id] is a C `uint32_t`; [key_type],
     * [purpose], [security_level] are DPP discriminant bytes; [public_key_data] is a
     * [com.sun.jna.Memory] of [public_key_data_len] bytes; [disabled_at] is a u64 (0 = enabled).
     */
    fun dash_sdk_identity_public_key_create_from_data(
        key_id: Int,
        key_type: Byte,
        purpose: Byte,
        security_level: Byte,
        public_key_data: Pointer,
        public_key_data_len: NativeLong,
        read_only: Boolean,
        disabled_at: Long
    ): DashSDKResultNative

    /**
     * Build an identity handle from its components (process-local; no network).
     * Returns a [DashSDKResult] with an identity handle; free it with [dash_sdk_identity_destroy].
     * [identity_id] is a [com.sun.jna.Memory] of 32 bytes; [public_keys] is a [com.sun.jna.Memory]
     * holding a contiguous array of [DashSDKPublicKeyDataNative] structs (length [public_keys_count]);
     * [balance] and [revision] are u64.
     *
     * For registration the [identity_id] is a placeholder — the IdentityCreate transition
     * derives the real on-chain id from the asset-lock proof (see IDENTITY_REGISTRATION_DESIGN.md §0.5).
     */
    fun dash_sdk_identity_create_from_components(
        identity_id: Pointer,
        public_keys: Pointer,
        public_keys_count: NativeLong,
        balance: Long,
        revision: Long
    ): DashSDKResultNative

    /**
     * Register an identity, funding it with an InstantSend-locked asset lock, and wait for
     * confirmation. Returns a [DashSDKResult] with the **confirmed** identity handle (its
     * on-chain id is derived from the asset-lock proof); free it with [dash_sdk_identity_destroy].
     *
     * [identity_handle] is the locally-built identity (see [dash_sdk_identity_create_from_components]);
     * [instant_lock_bytes]/[transaction_bytes] are the serialized InstantLock + funding tx
     * ([com.sun.jna.Memory]); [output_index] is the asset-lock output (C `uint32_t`);
     * [private_key] is a [com.sun.jna.Memory] of 32 bytes (the asset-lock output key);
     * [signer_handle] signs the IdentityCreate transition; [put_settings] may be null for defaults.
     */
    fun dash_sdk_identity_put_to_platform_with_instant_lock_and_wait(
        sdk_handle: Pointer,
        identity_handle: Pointer,
        instant_lock_bytes: Pointer,
        instant_lock_len: NativeLong,
        transaction_bytes: Pointer,
        transaction_len: NativeLong,
        output_index: Int,
        private_key: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?
    ): DashSDKResultNative

    /**
     * Register an identity funded by a ChainLock-confirmed asset lock, and wait for
     * confirmation. Returns a [DashSDKResult] with the confirmed identity handle (free with
     * [dash_sdk_identity_destroy]). [core_chain_locked_height] is a C `uint32_t`; [out_point]
     * is a [com.sun.jna.Memory] of 36 bytes (txid + vout); [private_key] is a 32-byte
     * [com.sun.jna.Memory]; [put_settings] may be null for defaults.
     */
    fun dash_sdk_identity_put_to_platform_with_chain_lock_and_wait(
        sdk_handle: Pointer,
        identity_handle: Pointer,
        core_chain_locked_height: Int,
        out_point: Pointer,
        private_key: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?
    ): DashSDKResultNative

    /**
     * Top up an existing identity's balance from an InstantSend-locked asset lock, and wait
     * for confirmation. No identity signer needed — the asset-lock key authorises the top-up.
     * Returns a [DashSDKResult] with the confirmed identity handle (free with
     * [dash_sdk_identity_destroy]). Args mirror
     * [dash_sdk_identity_put_to_platform_with_instant_lock_and_wait] minus the signer.
     */
    fun dash_sdk_identity_topup_with_instant_lock_and_wait(
        sdk_handle: Pointer,
        identity_handle: Pointer,
        instant_lock_bytes: Pointer,
        instant_lock_len: NativeLong,
        transaction_bytes: Pointer,
        transaction_len: NativeLong,
        output_index: Int,
        private_key: Pointer,
        put_settings: Pointer?
    ): DashSDKResultNative

    /**
     * Transfer credits from one identity to another. Returns a [DashSDKResult] with a
     * `*mut DashSDKTransferCreditsResult` (see [DashSDKTransferCreditsResultNative]); free it
     * with [dash_sdk_transfer_credits_result_free]. [to_identity_id] is the base58 recipient id;
     * [public_key_id] selects the signing key; [signer_handle] signs the transition;
     * [put_settings] may be null for defaults.
     */
    fun dash_sdk_identity_transfer_credits(
        sdk_handle: Pointer,
        from_identity_handle: Pointer,
        to_identity_id: String,
        amount: Long,
        public_key_id: Int,
        signer_handle: Pointer,
        put_settings: Pointer?
    ): DashSDKResultNative

    /** Free a [DashSDKTransferCreditsResult] from [dash_sdk_identity_transfer_credits]. */
    fun dash_sdk_transfer_credits_result_free(result: Pointer)

    /**
     * Withdraw credits from an identity to a Core [address]. Returns a [DashSDKResult] with the
     * new balance as a C string. [amount] is in credits; [core_fee_per_byte] (0 = default);
     * [public_key_id] selects the signing key; [signer_handle] signs the transition;
     * [put_settings] may be null for defaults.
     */
    fun dash_sdk_identity_withdraw(
        sdk_handle: Pointer,
        identity_handle: Pointer,
        address: String,
        amount: Long,
        core_fee_per_byte: Int,
        public_key_id: Int,
        signer_handle: Pointer,
        put_settings: Pointer?
    ): DashSDKResultNative

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
     * Fetch multiple data contracts by their IDs. Header:
     * `dash_sdk_data_contracts_fetch_many(const SDKHandle *, const char *contract_ids)`.
     * [contract_ids] is a comma-separated list (or JSON array) of Base58 contract IDs.
     * Returns a [DashSDKResult] with a JSON string (contract IDs → data contracts).
     */
    fun dash_sdk_data_contracts_fetch_many(sdk_handle: Pointer, contract_ids: String): DashSDKResultNative

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
    // Document write-path (state transitions) — external-signer pattern.
    // The leading SDKHandle, signing key (IdentityPublicKeyHandle*) and SignerHandle*
    // are passed as opaque [Pointer]s. The optional token_payment_info and
    // state_transition_creation_options are NULL (not modeled); put_settings is the
    // shared DashSDKPutSettings pointer (null = FFI defaults). All return a
    // [DashSDKResultNative]; the `_and_wait` variants' data is a confirmed DocumentHandle.
    // -------------------------------------------------------------------------

    /**
     * Create a new document (process-local; builds a DocumentHandle, no broadcast). Header:
     * `dash_sdk_document_create(SDKHandle*, const DashSDKDocumentCreateParams*)`.
     * Returns a [DashSDKResult] whose data is a heap [DashSDKDocumentCreateResultNative]
     * (a DocumentHandle + 32 bytes of entropy). Free that result with
     * [dash_sdk_document_create_result_free].
     */
    fun dash_sdk_document_create(
        sdk_handle: Pointer,
        params: DashSDKDocumentCreateParamsNative
    ): DashSDKResultNative

    /** Free a [DashSDKDocumentCreateResultNative] returned by [dash_sdk_document_create]. */
    fun dash_sdk_document_create_result_free(result: Pointer)

    /**
     * Build a DocumentHandle from explicit parameters (process-local; no broadcast). Header:
     * `dash_sdk_document_make_handle(const DashSDKDocumentHandleParams*)`.
     * Returns a [DashSDKResult] whose data is a heap DocumentHandle (free with
     * [dash_sdk_document_free] / [dash_sdk_document_handle_destroy]).
     */
    fun dash_sdk_document_make_handle(
        params: DashSDKDocumentHandleParamsNative
    ): DashSDKResultNative

    /**
     * Mutate a DocumentHandle's properties from a JSON object. Header:
     * `struct DashSDKError *dash_sdk_document_set_properties(DocumentHandle*, const char*)`.
     * Returns a `DashSDKError *` (null on success); free a non-null error with
     * [dash_sdk_error_free].
     */
    fun dash_sdk_document_set_properties(
        document_handle: Pointer,
        properties_json: String
    ): Pointer?

    /**
     * Free a DocumentHandle. Header: `void dash_sdk_document_free(DocumentHandle*)`.
     * Pairs with handles produced by [dash_sdk_document_create] / [dash_sdk_document_make_handle]
     * and the `_and_wait` write results.
     */
    fun dash_sdk_document_free(document_handle: Pointer)

    /**
     * Destroy a DocumentHandle. Header: `void dash_sdk_document_handle_destroy(DocumentHandle*)`.
     * Functional sibling of [dash_sdk_document_free] (both take a `DocumentHandle*` and return
     * `void`); the service uses [dash_sdk_document_free] uniformly.
     */
    fun dash_sdk_document_handle_destroy(document_handle: Pointer)

    /**
     * Put a document to platform and wait for confirmation. Header:
     * `dash_sdk_document_put_to_platform_and_wait(SDKHandle*, DocumentHandle*, data_contract_id,
     * document_type_name, const uint8_t (*entropy)[32], IdentityPublicKeyHandle*, SignerHandle*,
     * DashSDKTokenPaymentInfo*, DashSDKPutSettings*, DashSDKStateTransitionCreationOptions*)`.
     * [entropy] is a 32-byte buffer. Returns a [DashSDKResult] with a confirmed DocumentHandle.
     */
    fun dash_sdk_document_put_to_platform_and_wait(
        sdk_handle: Pointer,
        document_handle: Pointer,
        data_contract_id: String,
        document_type_name: String,
        entropy: Pointer,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Replace a document on platform and wait for confirmation. Header:
     * `dash_sdk_document_replace_on_platform_and_wait(SDKHandle*, DocumentHandle*, data_contract_id,
     * document_type_name, IdentityPublicKeyHandle*, SignerHandle*, ...)`.
     * Returns a [DashSDKResult] with a confirmed DocumentHandle.
     */
    fun dash_sdk_document_replace_on_platform_and_wait(
        sdk_handle: Pointer,
        document_handle: Pointer,
        data_contract_id: String,
        document_type_name: String,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Delete a document and wait for confirmation. Header:
     * `dash_sdk_document_delete_and_wait(SDKHandle*, document_id, owner_id, data_contract_id,
     * document_type_name, IdentityPublicKeyHandle*, SignerHandle*, ...)`.
     * Returns a [DashSDKResult].
     */
    fun dash_sdk_document_delete_and_wait(
        sdk_handle: Pointer,
        document_id: String,
        owner_id: String,
        data_contract_id: String,
        document_type_name: String,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Transfer a document to another identity and wait for confirmation. Header:
     * `dash_sdk_document_transfer_to_identity_and_wait(SDKHandle*, DocumentHandle*, recipient_id,
     * data_contract_id, document_type_name, IdentityPublicKeyHandle*, SignerHandle*, ...)`.
     * Returns a [DashSDKResult] with a confirmed DocumentHandle.
     */
    fun dash_sdk_document_transfer_to_identity_and_wait(
        sdk_handle: Pointer,
        document_handle: Pointer,
        recipient_id: String,
        data_contract_id: String,
        document_type_name: String,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Purchase a document at a given price and wait for confirmation. Header:
     * `dash_sdk_document_purchase_and_wait(SDKHandle*, DocumentHandle*, data_contract_id,
     * document_type_name, uint64_t price, purchaser_id, IdentityPublicKeyHandle*, SignerHandle*, ...)`.
     * Returns a [DashSDKResult] with a confirmed DocumentHandle.
     */
    fun dash_sdk_document_purchase_and_wait(
        sdk_handle: Pointer,
        document_handle: Pointer,
        data_contract_id: String,
        document_type_name: String,
        price: Long,
        purchaser_id: String,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Update a document's price and wait for confirmation. Header:
     * `dash_sdk_document_update_price_of_document_and_wait(SDKHandle*, DocumentHandle*,
     * data_contract_id, document_type_name, uint64_t price, IdentityPublicKeyHandle*,
     * SignerHandle*, ...)`.
     * Returns a [DashSDKResult] with a confirmed DocumentHandle.
     */
    fun dash_sdk_document_update_price_of_document_and_wait(
        sdk_handle: Pointer,
        document_handle: Pointer,
        data_contract_id: String,
        document_type_name: String,
        price: Long,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        token_payment_info: Pointer?,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    // -------------------------------------------------------------------------
    // DPNS queries
    // -------------------------------------------------------------------------

    /** Resolve a DPNS name to an identity ID. Returns JSON string result. */
    fun dash_sdk_dpns_resolve(handle: Pointer, name: String): DashSDKResultNative

    /** Check if a DPNS name is available. Returns bool result. */
    fun dash_sdk_dpns_check_availability(handle: Pointer, name: String): DashSDKResultNative

    /** Search DPNS names by prefix. Returns JSON array result. */
    fun dash_sdk_dpns_search(handle: Pointer, prefix: String, limit: Int): DashSDKResultNative

    /**
     * Get all contested DPNS usernames. [limit] is a C `uint32_t` (unsigned 32-bit);
     * [start_after] is a nullable cursor (label to page after). Returns a JSON string result.
     */
    fun dash_sdk_dpns_get_all_contested_usernames(
        handle: Pointer,
        limit: Int,
        start_after: String?
    ): DashSDKResultNative

    /**
     * Get all contested DPNS usernames where [identity_id] is a contender.
     * [limit] is a C `uint32_t`. Returns a JSON string result.
     */
    fun dash_sdk_dpns_get_contested_usernames_by_identity(
        handle: Pointer,
        identity_id: String,
        limit: Int
    ): DashSDKResultNative

    /**
     * Get the vote state for a contested DPNS username [label]. [limit] is a C `uint32_t`.
     * Returns a JSON string result.
     */
    fun dash_sdk_dpns_get_contested_vote_state(
        handle: Pointer,
        label: String,
        limit: Int
    ): DashSDKResultNative

    /**
     * Get all contested DPNS usernames that [identity_id] has voted on. [limit] is a C
     * `uint32_t`; [offset] is a C `uint16_t` (unsigned 16-bit). Returns a JSON string result.
     */
    fun dash_sdk_dpns_get_identity_votes(
        handle: Pointer,
        identity_id: String,
        limit: Int,
        offset: Int
    ): DashSDKResultNative

    /**
     * Get the DPNS usernames owned by [identity_id]. [limit] is a C `uint32_t`.
     * Returns a JSON string result.
     */
    fun dash_sdk_dpns_get_usernames(
        handle: Pointer,
        identity_id: String,
        limit: Int
    ): DashSDKResultNative

    /**
     * Validate a DPNS [name] and return a human-readable validation message.
     * Process-local — no SDK handle, no network. Returns a [DashSDKResult] with a C string.
     */
    fun dash_sdk_dpns_get_validation_message(name: String): DashSDKResultNative

    /**
     * Normalize a DPNS [name] (case-folding / homoglyph normalization).
     * Process-local — no SDK handle, no network. Returns a [DashSDKResult] with a C string.
     */
    fun dash_sdk_dpns_normalize_username(name: String): DashSDKResultNative

    /**
     * Check whether [name] is a valid DPNS username. Process-local — no SDK handle.
     * Returns a RAW `int32_t` (NOT a [DashSDKResult]): -1 on null/error, 0 invalid, 1 valid.
     * Do not route through [ResultUnwrapper]; callers interpret `== 1`.
     */
    fun dash_sdk_dpns_is_valid_username(name: String): Int

    /**
     * Check whether [name] is a contested DPNS username. Process-local — no SDK handle.
     * Returns a RAW `int32_t` (NOT a [DashSDKResult]): -1 on null/error, 0 not-contested,
     * 1 contested. Do not route through [ResultUnwrapper]; callers interpret `== 1`.
     */
    fun dash_sdk_dpns_is_contested_username(name: String): Int

    /**
     * Get current DPNS contests (active vote polls) ending within [start_time]..[end_time]
     * (ms). [limit] is a C `uint16_t` (unsigned 16-bit). Header:
     * `struct DashSDKNameTimestampList *dash_sdk_dpns_get_current_contests(const SDKHandle *, uint64_t, uint64_t, uint16_t)`.
     * Returns a heap pointer to a [DashSDKNameTimestampListNative] (or null) — read it, then
     * free with [dash_sdk_name_timestamp_list_free]. (NOT a DashSDKResult.)
     */
    fun dash_sdk_dpns_get_current_contests(
        handle: Pointer,
        start_time: Long,
        end_time: Long,
        limit: Int
    ): Pointer?

    /** Free a [DashSDKNameTimestampListNative] returned by [dash_sdk_dpns_get_current_contests]. */
    fun dash_sdk_name_timestamp_list_free(list: Pointer)

    /**
     * All unresolved contested DPNS usernames with full contest state. Header:
     * `struct DashSDKContestedNamesList *dash_sdk_dpns_get_contested_non_resolved_usernames(const SDKHandle *, uint32_t)`.
     * Returns a heap pointer to a [DashSDKContestedNamesListNative] (or null) — read it, then
     * free with [dash_sdk_contested_names_list_free]. (NOT a DashSDKResult.)
     */
    fun dash_sdk_dpns_get_contested_non_resolved_usernames(handle: Pointer, limit: Int): Pointer?

    /**
     * Unresolved contested usernames an identity contends in, with full contest state. Header:
     * `struct DashSDKContestedNamesList *dash_sdk_dpns_get_non_resolved_contests_for_identity(const SDKHandle *, const char *, uint32_t)`.
     * Returns a heap pointer to a [DashSDKContestedNamesListNative] (or null); free with
     * [dash_sdk_contested_names_list_free].
     */
    fun dash_sdk_dpns_get_non_resolved_contests_for_identity(
        handle: Pointer,
        identity_id: String,
        limit: Int
    ): Pointer?

    /**
     * Free a [DashSDKContestedNamesListNative] (and its nested contenders) returned by the two
     * contested-non-resolved-usernames queries.
     */
    fun dash_sdk_contested_names_list_free(list: Pointer)

    /**
     * Register a DPNS username (preorder + domain) in one operation. Header:
     * `struct DashSDKResult dash_sdk_dpns_register_name(const SDKHandle *, const char *label,
     * const void *identity, const void *identity_public_key, const void *signer)`.
     * [identity] is a raw `IdentityHandle`, [identity_public_key] a raw `IdentityPublicKeyHandle`
     * (the signing key), [signer] a raw `SignerHandle` — all `Pointer`. Returns a
     * [DashSDKResult] whose data is a `DpnsRegistrationResult *` (see
     * [DpnsRegistrationResultNative]); free with [dash_sdk_dpns_registration_result_free].
     */
    fun dash_sdk_dpns_register_name(
        handle: Pointer,
        label: String,
        identity: Pointer,
        identity_public_key: Pointer,
        signer: Pointer
    ): DashSDKResultNative

    /** Free a [DpnsRegistrationResultNative] returned by [dash_sdk_dpns_register_name]. */
    fun dash_sdk_dpns_registration_result_free(result: Pointer)

    // -------------------------------------------------------------------------
    // Contested-resource + voting queries (read-path; return DashSDKResult JSON string)
    // -------------------------------------------------------------------------

    /**
     * Get the contested-resource votes cast by [identity_id]. [limit]/[offset] are C
     * `uint32_t` (unsigned 32-bit); [order_ascending] orders by resource. JSON string result.
     */
    fun dash_sdk_contested_resource_get_identity_votes(
        handle: Pointer,
        identity_id: String,
        limit: Int,
        offset: Int,
        order_ascending: Boolean
    ): DashSDKResultNative

    /**
     * Get contested resources for an index. [start_index_values_json]/[end_index_values_json]
     * are JSON-array cursors; [count] is a C `uint32_t`. JSON string result.
     */
    fun dash_sdk_contested_resource_get_resources(
        handle: Pointer,
        contract_id: String,
        document_type_name: String,
        index_name: String,
        start_index_values_json: String,
        end_index_values_json: String,
        count: Int,
        order_ascending: Boolean
    ): DashSDKResultNative

    /**
     * Get the vote state for a contested resource. [result_type] is a C `uint8_t`
     * (0=documents, 1=vote-tally, …); [count] is a C `uint32_t`. JSON string result.
     */
    fun dash_sdk_contested_resource_get_vote_state(
        handle: Pointer,
        contract_id: String,
        document_type_name: String,
        index_name: String,
        index_values_json: String,
        result_type: Byte,
        allow_include_locked_and_abstaining_vote_tally: Boolean,
        count: Int
    ): DashSDKResultNative

    /**
     * Get the voters for a contestant on a contested resource. [count] is a C `uint32_t`.
     * JSON string result.
     */
    fun dash_sdk_contested_resource_get_voters_for_identity(
        handle: Pointer,
        contract_id: String,
        document_type_name: String,
        index_name: String,
        index_values_json: String,
        contestant_id: String,
        count: Int,
        order_ascending: Boolean
    ): DashSDKResultNative

    /**
     * Get vote polls by end date. [start_time_ms]/[end_time_ms] are C `uint64_t`;
     * [limit]/[offset] are C `uint32_t`. JSON string result.
     */
    fun dash_sdk_voting_get_vote_polls_by_end_date(
        handle: Pointer,
        start_time_ms: Long,
        start_time_included: Boolean,
        end_time_ms: Long,
        end_time_included: Boolean,
        limit: Int,
        offset: Int,
        ascending: Boolean
    ): DashSDKResultNative

    // -------------------------------------------------------------------------
    // Group queries (read-path; return DashSDKResult JSON string)
    // -------------------------------------------------------------------------

    /**
     * Get info for a group at [group_contract_position] in a contract. [group_contract_position]
     * is a C `uint16_t` (unsigned 16-bit). JSON string result.
     */
    fun dash_sdk_group_get_info(
        handle: Pointer,
        contract_id: String,
        group_contract_position: Int
    ): DashSDKResultNative

    /**
     * Get infos for all groups in a contract. [start_at_position] is a nullable cursor;
     * [limit] is a C `uint32_t`. JSON string result.
     */
    fun dash_sdk_group_get_infos(
        handle: Pointer,
        start_at_position: String?,
        limit: Int
    ): DashSDKResultNative

    /**
     * Get group actions. [group_contract_position] is a C `uint16_t`; [status] is a C `uint8_t`;
     * [start_at_action_id] is a nullable cursor; [limit] is a C `uint16_t`. JSON string result.
     */
    fun dash_sdk_group_get_actions(
        handle: Pointer,
        contract_id: String,
        group_contract_position: Int,
        status: Byte,
        start_at_action_id: String?,
        limit: Int
    ): DashSDKResultNative

    /**
     * Get the signers of a group action. [group_contract_position] is a C `uint16_t`;
     * [status] is a C `uint8_t`. JSON string result.
     */
    fun dash_sdk_group_get_action_signers(
        handle: Pointer,
        contract_id: String,
        group_contract_position: Int,
        status: Byte,
        action_id: String
    ): DashSDKResultNative

    // -------------------------------------------------------------------------
    // Evonode queries (read-path; return DashSDKResult JSON string)
    // -------------------------------------------------------------------------

    /**
     * Get proposed epoch blocks for a set of evonode IDs. [epoch] is a C `uint32_t`;
     * [ids_json] is a JSON array of evonode (proTxHash) IDs. JSON string result.
     */
    fun dash_sdk_evonode_get_proposed_epoch_blocks_by_ids(
        handle: Pointer,
        epoch: Int,
        ids_json: String
    ): DashSDKResultNative

    /**
     * Get proposed epoch blocks over a range. [epoch]/[limit] are C `uint32_t`;
     * [start_after] and [start_at] are nullable cursors. JSON string result.
     */
    fun dash_sdk_evonode_get_proposed_epoch_blocks_by_range(
        handle: Pointer,
        epoch: Int,
        limit: Int,
        start_after: String?,
        start_at: String?
    ): DashSDKResultNative

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
    // Token write-path (state transitions) — external-signer pattern. Each takes the
    // 32-byte transition_owner_id (uint8_t* → 32-byte Memory), a *Params struct by
    // pointer, the signing IdentityPublicKeyHandle, the SignerHandle, and the optional
    // put_settings / state_transition_creation_options (always pass NULL for st-options).
    // On the Rust side these return DashSDKResult::success(null) — no data payload — so
    // the service layer unwraps via ResultUnwrapper.unwrapVoid (error-only check).
    // -------------------------------------------------------------------------

    /**
     * Mint tokens to an identity and wait for confirmation. Header:
     * `dash_sdk_token_mint(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenMintParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_mint(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenMintParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Burn tokens from an identity and wait for confirmation. Header:
     * `dash_sdk_token_burn(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenBurnParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_burn(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenBurnParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Transfer tokens to another identity and wait for confirmation. Header:
     * `dash_sdk_token_transfer(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenTransferParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_transfer(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenTransferParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Freeze a token for an identity and wait for confirmation. Header:
     * `dash_sdk_token_freeze(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenFreezeParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_freeze(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenFreezeParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Unfreeze a token for an identity and wait for confirmation. Header:
     * `dash_sdk_token_unfreeze(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenFreezeParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Reuses [DashSDKTokenFreezeParamsNative] (the header's `DashSDKTokenFreezeParams`).
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_unfreeze(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenFreezeParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Claim a token distribution (pre-programmed or perpetual) and wait for confirmation.
     * Header: `dash_sdk_token_claim(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenClaimParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_claim(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenClaimParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Set a token's direct-purchase price (single or tiered) and wait for confirmation.
     * Header: `dash_sdk_token_set_price(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenSetPriceParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_set_price(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenSetPriceParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Purchase tokens directly at the agreed price and wait for confirmation. Header:
     * `dash_sdk_token_purchase(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenPurchaseParams*, const IdentityPublicKeyHandle*, const SignerHandle*,
     * const DashSDKPutSettings*, const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_purchase(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenPurchaseParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Destroy a frozen identity's token funds and wait for confirmation. Header:
     * `dash_sdk_token_destroy_frozen_funds(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenDestroyFrozenFundsParams*, const IdentityPublicKeyHandle*,
     * const SignerHandle*, const DashSDKPutSettings*,
     * const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_destroy_frozen_funds(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenDestroyFrozenFundsParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Perform a token emergency action (pause/resume) and wait for confirmation. Header:
     * `dash_sdk_token_emergency_action(SDKHandle*, const uint8_t *transition_owner_id,
     * const DashSDKTokenEmergencyActionParams*, const IdentityPublicKeyHandle*,
     * const SignerHandle*, const DashSDKPutSettings*,
     * const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_emergency_action(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenEmergencyActionParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

    /**
     * Update a contract's token configuration and wait for confirmation. Header:
     * `dash_sdk_token_update_contract_token_configuration(SDKHandle*,
     * const uint8_t *transition_owner_id, const DashSDKTokenConfigUpdateParams*,
     * const IdentityPublicKeyHandle*, const SignerHandle*, const DashSDKPutSettings*,
     * const DashSDKStateTransitionCreationOptions*)`.
     * Returns a [DashSDKResult] with no data payload on success.
     */
    fun dash_sdk_token_update_contract_token_configuration(
        sdk_handle: Pointer,
        transition_owner_id: Pointer,
        params: DashSDKTokenConfigUpdateParamsNative,
        identity_public_key_handle: Pointer,
        signer_handle: Pointer,
        put_settings: Pointer?,
        state_transition_creation_options: Pointer?
    ): DashSDKResultNative

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
     * Fetch GroveDB path elements. Header:
     * `dash_sdk_system_get_path_elements(const SDKHandle *, const char *path_json, const char *keys_json)`.
     * [path_json] is a JSON array of path elements (hex-encoded byte arrays); [keys_json] is
     * a JSON array of keys (hex-encoded byte arrays).
     * Returns a [DashSDKResult] with a JSON array string of elements (or null if not found).
     */
    fun dash_sdk_system_get_path_elements(
        handle: Pointer,
        path_json: String,
        keys_json: String
    ): DashSDKResultNative

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
    // Pure utilities (no SDK handle, no network — process-local)
    // -------------------------------------------------------------------------

    /**
     * Convert a base58 string to hex. Header:
     * `struct DashSDKResult dash_sdk_utils_base58_to_hex(const char *base58_string)`.
     * Returns a [DashSDKResult] with a hex-encoded C string.
     */
    fun dash_sdk_utils_base58_to_hex(base58_string: String): DashSDKResultNative

    /**
     * Convert a hex string to base58. Header:
     * `struct DashSDKResult dash_sdk_utils_hex_to_base58(const char *hex_string)`.
     * Returns a [DashSDKResult] with a base58-encoded C string.
     */
    fun dash_sdk_utils_hex_to_base58(hex_string: String): DashSDKResultNative

    /**
     * Validate whether [string] is valid base58. Header:
     * `uint8_t dash_sdk_utils_is_valid_base58(const char *string)`.
     * Returns a RAW `uint8_t` (1 = valid, 0 = invalid) — NOT a [DashSDKResult]; do not route
     * through [ResultUnwrapper]. Mapped as [Byte]; callers interpret `!= 0`.
     */
    fun dash_sdk_utils_is_valid_base58(string: String): Byte

    /**
     * Encode a P2PKH scriptPubKey as a bech32m platform address (DIP-18). Header:
     * `struct DashSDKResult dash_sdk_encode_platform_address(const uint8_t *script_pubkey, uint32_t script_len, FFINetwork network)`.
     * [script_pubkey] is a [com.sun.jna.Memory] holding [script_len] raw bytes (a 25-byte
     * P2PKH script); [script_len] is a C `uint32_t` (unsigned 32-bit). [network] is the
     * `FFINetwork` enum value (see [FFINetwork]; differs from [DashSDKNetwork]).
     * Returns a [DashSDKResult] with the bech32m address as a C string.
     */
    fun dash_sdk_encode_platform_address(
        script_pubkey: Pointer,
        script_len: Int,
        network: Int
    ): DashSDKResultNative

    /**
     * Format a raw GroveDB proof as a human-readable string. Header:
     * `struct DashSDKResult dash_sdk_format_grovedb_proof(const uint8_t *proof_bytes, uint32_t proof_len)`.
     * [proof_bytes] is a [com.sun.jna.Memory] holding [proof_len] raw bytes; [proof_len] is a
     * C `uint32_t` (unsigned 32-bit).
     * Returns a [DashSDKResult] with a tree-structured visualization as a C string.
     */
    fun dash_sdk_format_grovedb_proof(proof_bytes: Pointer, proof_len: Int): DashSDKResultNative

    // -------------------------------------------------------------------------
    // Signing — external-signer infrastructure (rs-sdk-ffi; no platform-wallet/unified)
    //
    // All `uintptr_t` lengths below are bound as [NativeLong] (pointer-width on every
    // target ABI). The callback trampolines (Rust → JVM) are held alive by the owning
    // Kotlin signer; see org.dash.sdk.signing.KeystoreSigner.
    // -------------------------------------------------------------------------

    /**
     * Create an external signer backed by Kotlin [Callback] trampolines. Returns a raw
     * `*mut SignerHandle` (NOT a [DashSDKResult]); null on failure. Free with
     * [dash_sdk_signer_destroy]. Header:
     * `struct SignerHandle *dash_sdk_signer_create(SignAsyncCallback, CanSignCallback, DestroyCallback)`.
     *
     * The three callback objects MUST be kept strongly referenced for the life of the
     * returned handle — JNA frees the native trampoline when the Callback is GC'd.
     */
    fun dash_sdk_signer_create(
        sign_async_callback: SignAsyncCallback,
        can_sign_callback: CanSignCallback,
        destroy_callback: DestroyCallback
    ): Pointer?

    /**
     * Sibling to [dash_sdk_signer_create] taking an opaque `ctx` forwarded to every
     * callback. Kotlin callbacks capture state directly, so [dash_sdk_signer_create]
     * (no ctx) is preferred; bound for completeness.
     */
    fun dash_sdk_signer_create_with_ctx(
        ctx: Pointer?,
        sign_async_callback: SignAsyncCallback,
        can_sign_callback: CanSignCallback,
        destroy_callback: DestroyCallback
    ): Pointer?

    /** Destroy a signer handle from [dash_sdk_signer_create] / [dash_sdk_signer_create_from_private_key]. */
    fun dash_sdk_signer_destroy(handle: Pointer)

    /**
     * Synchronous key-availability check. Routes through the signer's `can_sign` vtable
     * entry (i.e. invokes the owning signer's [CanSignCallback] for vtable signers).
     * [pubkey_bytes] is a [com.sun.jna.Memory] of [pubkey_len] bytes; [key_type] is the
     * DPP KeyType discriminant byte.
     */
    fun dash_sdk_signer_can_sign(
        signer: Pointer,
        pubkey_bytes: Pointer,
        pubkey_len: NativeLong,
        key_type: Byte
    ): Boolean

    /**
     * Create a throwaway signer from a raw private key (v1 sign primitive). Returns a
     * [DashSDKResult] whose `data` is a `*mut SignerHandle`. [private_key] is a
     * [com.sun.jna.Memory] of [private_key_len] (32) bytes; [network] is the `FFINetwork`
     * value (see [FFINetwork]).
     */
    fun dash_sdk_signer_create_from_private_key(
        private_key: Pointer,
        private_key_len: NativeLong,
        network: Int
    ): DashSDKResultNative

    /**
     * Sign [data] with a signer handle. Returns a [DashSDKResult] whose `data` is a
     * `*mut DashSDKSignature` (see [DashSDKSignatureNative]); free it with
     * [dash_sdk_signature_free]. [data] is a [com.sun.jna.Memory] of [data_len] bytes.
     */
    fun dash_sdk_signer_sign(signer_handle: Pointer, data: Pointer, data_len: NativeLong): DashSDKResultNative

    /** Free a [DashSDKSignature] returned by [dash_sdk_signer_sign]. */
    fun dash_sdk_signature_free(signature: Pointer)

    /**
     * Deliver a signature (or error) back to the SDK for an in-flight async sign request.
     * [completion_ctx] MUST be the exact pointer handed to the [SignAsyncCallback].
     * On success pass [signature] (a [com.sun.jna.Memory]) + [signature_len] and null
     * [error_message]; on failure pass null/0 and a non-null [error_message].
     */
    fun dash_sdk_sign_async_completion(
        completion_ctx: Pointer?,
        signature: Pointer?,
        signature_len: NativeLong,
        error_message: String?
    )

    // -------------------------------------------------------------------------
    // Crypto helpers (process-local; no SDK handle)
    // -------------------------------------------------------------------------

    /**
     * Derive the public-key bytes (hex) for a private key. Header:
     * `DashSDKResult dash_sdk_public_key_data_from_private_key_data(const char *private_key_hex, uint8_t key_type, FFINetwork network)`.
     * Returns a [DashSDKResult] with a hex C string. [key_type] is the DPP KeyType byte.
     */
    fun dash_sdk_public_key_data_from_private_key_data(
        private_key_hex: String,
        key_type: Byte,
        network: Int
    ): DashSDKResultNative

    /**
     * Validate that a private key corresponds to a public key. Header:
     * `DashSDKResult dash_sdk_validate_private_key_for_public_key(const char *private_key_hex, const char *public_key_hex, uint8_t key_type, FFINetwork network)`.
     * Returns a [DashSDKResult] with a status C string.
     */
    fun dash_sdk_validate_private_key_for_public_key(
        private_key_hex: String,
        public_key_hex: String,
        key_type: Byte,
        network: Int
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
// JNA Callback mappings for the signer function-pointer typedefs in rs-sdk-ffi.h
//
// Each is a single-method [Callback] (JNA builds one native trampoline per instance).
// The owning signer MUST keep its callback instances strongly referenced for the life
// of the SignerHandle, or JNA will free the trampoline and the native side will crash.
// ---------------------------------------------------------------------------

/**
 * `typedef void (*SignAsyncCallback)(const void *signer, const uint8_t *pubkey_bytes,
 * uintptr_t pubkey_len, uint8_t key_type, const uint8_t *data, uintptr_t data_len,
 * void *completion_ctx, SignCompletionCallback completion)`.
 *
 * Implementations look up the private key for [pubkeyBytes], sign [data], and deliver the
 * result via [DashSdkFfi.dash_sdk_sign_async_completion] using [completionCtx] (the raw
 * [completion] pointer is left opaque — the completion helper drives it). May fire from
 * any native worker thread.
 */
fun interface SignAsyncCallback : Callback {
    fun invoke(
        signer: Pointer?,
        pubkeyBytes: Pointer?,
        pubkeyLen: NativeLong,
        keyType: Byte,
        data: Pointer?,
        dataLen: NativeLong,
        completionCtx: Pointer?,
        completion: Pointer?
    )
}

/**
 * `typedef bool (*CanSignCallback)(const void *signer, const uint8_t *pubkey_bytes,
 * uintptr_t pubkey_len, uint8_t key_type)`. Synchronous availability check.
 */
fun interface CanSignCallback : Callback {
    fun invoke(signer: Pointer?, pubkeyBytes: Pointer?, pubkeyLen: NativeLong, keyType: Byte): Boolean
}

/** `typedef void (*DestroyCallback)(void *signer)`. Invoked once when the handle is destroyed. */
fun interface DestroyCallback : Callback {
    fun invoke(signer: Pointer?)
}

// ---------------------------------------------------------------------------
// JNA Structure mappings for C structs in dash_sdk_ffi.h
// ---------------------------------------------------------------------------

/**
 * Maps to `struct DashSDKSignature` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_signer_sign].
 *
 * 64-bit layout: `signature: uint8_t* (8)` + `signature_len: uintptr_t (8)` = 16 bytes.
 *
 * Usage: construct over the returned pointer, [read], copy [signature_len] bytes out of
 * [signature], then release the pointer with [DashSdkFfi.dash_sdk_signature_free].
 */
/**
 * Maps to `struct DashSDKPublicKeyData` in rs-sdk-ffi.h — one element of the array passed
 * to [DashSdkFfi.dash_sdk_identity_create_from_components].
 *
 * C 64-bit layout (JNA inserts the alignment padding automatically):
 *   id:             u8        @0  (1)
 *   purpose:        u8        @1  (1)
 *   security_level: u8        @2  (1)
 *   key_type:       u8        @3  (1)
 *   read_only:      bool      @4  (1) + 3 pad
 *   data:           u8*       @8  (8)
 *   data_len:       uintptr_t @16 (8)
 *   disabled_at:    u64       @24 (8)
 *   = 32 bytes
 *
 * Build a contiguous array via `DashSDKPublicKeyDataNative().toArray(n)`, fill each row
 * (with [data] pointing at a pinned [com.sun.jna.Memory] kept alive across the call) and
 * call [write] on each before passing `rows[0].pointer`. Field discriminants follow DPP
 * `repr(u8)` enums (KeyType 0=ECDSA_SECP256K1; Purpose 0=AUTHENTICATION;
 * SecurityLevel 0=MASTER,1=CRITICAL,2=HIGH,3=MEDIUM).
 */
@Structure.FieldOrder("id", "purpose", "security_level", "key_type", "read_only", "data", "data_len", "disabled_at")
class DashSDKPublicKeyDataNative : Structure {
    /** Key ID (0-255). */
    @JvmField var id: Byte = 0
    /** DPP Purpose discriminant. */
    @JvmField var purpose: Byte = 0
    /** DPP SecurityLevel discriminant. */
    @JvmField var security_level: Byte = 0
    /** DPP KeyType discriminant. */
    @JvmField var key_type: Byte = 0
    /** Whether the key is read-only. */
    @JvmField var read_only: Boolean = false
    /** Pointer to the public-key bytes (borrowed for the call). */
    @JvmField var data: Pointer? = null
    /** Length of [data]. */
    @JvmField var data_len: NativeLong = NativeLong(0)
    /** Disabled-at timestamp (0 = enabled). */
    @JvmField var disabled_at: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKPutSettings` in rs-sdk-ffi.h — optional tuning for the put-to-platform
 * entry points. Pass its [pointer] (after [write]) or null for all-defaults (every `0` field
 * means "use default").
 *
 * C 64-bit layout (JNA inserts the alignment padding):
 *   connect_timeout_ms:                    u64  @0  (8)
 *   timeout_ms:                            u64  @8  (8)
 *   retries:                               u32  @16 (4)
 *   ban_failed_address:                    bool @20 (1) + 3 pad
 *   identity_nonce_stale_time_s:           u64  @24 (8)
 *   user_fee_increase:                     u16  @32 (2)
 *   allow_signing_with_any_security_level: bool @34 (1)
 *   allow_signing_with_any_purpose:        bool @35 (1) + 4 pad
 *   wait_timeout_ms:                       u64  @40 (8)
 *   = 48 bytes
 *
 * The three `bool`s are mapped as [Byte] (not `Boolean`): `user_fee_increase` (u16) sits
 * immediately before two of them, so the 4-byte width JNA gives `Boolean` would push those
 * fields past their 1-byte C offsets. Write 0/1.
 */
@Structure.FieldOrder(
    "connect_timeout_ms",
    "timeout_ms",
    "retries",
    "ban_failed_address",
    "identity_nonce_stale_time_s",
    "user_fee_increase",
    "allow_signing_with_any_security_level",
    "allow_signing_with_any_purpose",
    "wait_timeout_ms"
)
class DashSDKPutSettingsNative : Structure {
    @JvmField var connect_timeout_ms: Long = 0
    @JvmField var timeout_ms: Long = 0
    @JvmField var retries: Int = 0
    @JvmField var ban_failed_address: Byte = 0
    @JvmField var identity_nonce_stale_time_s: Long = 0
    @JvmField var user_fee_increase: Short = 0
    @JvmField var allow_signing_with_any_security_level: Byte = 0
    @JvmField var allow_signing_with_any_purpose: Byte = 0
    @JvmField var wait_timeout_ms: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKTransferCreditsResult` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_identity_transfer_credits]. 64-bit layout: `sender_balance: u64 (8)` +
 * `receiver_balance: u64 (8)` = 16 bytes. Release with [DashSdkFfi.dash_sdk_transfer_credits_result_free].
 */
@Structure.FieldOrder("sender_balance", "receiver_balance")
class DashSDKTransferCreditsResultNative : Structure {
    /** Sender's final balance after the transfer. */
    @JvmField var sender_balance: Long = 0
    /** Receiver's final balance after the transfer. */
    @JvmField var receiver_balance: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

@Structure.FieldOrder("signature", "signature_len")
class DashSDKSignatureNative : Structure {
    /** Signature bytes (heap). */
    @JvmField var signature: Pointer? = null
    /** Length of [signature] (compact-recoverable ECDSA is 65 bytes). */
    @JvmField var signature_len: NativeLong = NativeLong(0)

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

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
 * Maps to `struct DashSDKDocumentCreateParams` in rs-sdk-ffi.h — passed **by pointer** to
 * [DashSdkFfi.dash_sdk_document_create].
 *
 * C 64-bit layout (auditor-verified via cc: size=32, offsets 0/8/16/24; four `const char*`):
 *   data_contract_id @0 (8) + document_type @8 (8) + owner_identity_id @16 (8)
 *   + properties_json @24 (8) = 32 bytes.
 */
@Structure.FieldOrder("data_contract_id", "document_type", "owner_identity_id", "properties_json")
class DashSDKDocumentCreateParamsNative : Structure() {
    /** Data contract ID (base58). */
    @JvmField var data_contract_id: String? = null
    /** Document type name. */
    @JvmField var document_type: String? = null
    /** Owner identity ID (base58). */
    @JvmField var owner_identity_id: String? = null
    /** JSON object of document properties. */
    @JvmField var properties_json: String? = null
}

/**
 * Maps to `struct DashSDKDocumentHandleParams` in rs-sdk-ffi.h — passed **by pointer** to
 * [DashSdkFfi.dash_sdk_document_make_handle].
 *
 * C 64-bit layout (auditor-verified via cc: size=48, offsets 0/8/16/24/32/40): five
 * `const char*` then a `uint64_t revision` (0 means no revision).
 */
@Structure.FieldOrder(
    "id",
    "data_contract_id",
    "document_type",
    "owner_identity_id",
    "properties_json",
    "revision"
)
class DashSDKDocumentHandleParamsNative : Structure() {
    /** Document ID (base58). */
    @JvmField var id: String? = null
    /** Data contract ID (base58). */
    @JvmField var data_contract_id: String? = null
    /** Document type name. */
    @JvmField var document_type: String? = null
    /** Owner identity ID (base58). */
    @JvmField var owner_identity_id: String? = null
    /** JSON object of document properties. */
    @JvmField var properties_json: String? = null
    /** Optional revision number (0 = no revision). */
    @JvmField var revision: Long = 0
}

// ---------------------------------------------------------------------------
// Token write-path parameter structs. Each is `repr(C)` in rs-sdk-ffi.h and passed
// **by pointer** to its dash_sdk_token_* entry point. `token_contract_id` (Base58
// const char*) and `serialized_contract`/`serialized_contract_len` are mutually
// exclusive; the Kotlin services pass the Base58 id and leave the serialized_* pair
// null/0. 32-byte identity ids are `const uint8_t*` (a 32-byte JNA Memory, nullable).
// uint16_t → Short, uint64_t → Long, uintptr_t → NativeLong, const char* → String?.
// ---------------------------------------------------------------------------

/**
 * Maps to `struct DashSDKTokenMintParams` in rs-sdk-ffi.h (7 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_mint].
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "recipient_id",
    "amount",
    "public_note"
)
class DashSDKTokenMintParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Recipient identity ID (32 raw bytes) — optional. */
    @JvmField var recipient_id: Pointer? = null
    /** Amount to mint (uint64_t). */
    @JvmField var amount: Long = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenBurnParams` in rs-sdk-ffi.h (6 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_burn].
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "amount",
    "public_note"
)
class DashSDKTokenBurnParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Amount to burn (uint64_t). */
    @JvmField var amount: Long = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenTransferParams` in rs-sdk-ffi.h (9 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_transfer].
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "recipient_id",
    "amount",
    "public_note",
    "private_encrypted_note",
    "shared_encrypted_note"
)
class DashSDKTokenTransferParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Recipient identity ID (32 raw bytes). */
    @JvmField var recipient_id: Pointer? = null
    /** Amount to transfer (uint64_t). */
    @JvmField var amount: Long = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
    /** Optional private encrypted note. */
    @JvmField var private_encrypted_note: String? = null
    /** Optional shared encrypted note. */
    @JvmField var shared_encrypted_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenFreezeParams` in rs-sdk-ffi.h (6 fields). Passed **by pointer**
 * to both [DashSdkFfi.dash_sdk_token_freeze] and [DashSdkFfi.dash_sdk_token_unfreeze]
 * (the header reuses one struct for "Token freeze/unfreeze parameters").
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "target_identity_id",
    "public_note"
)
class DashSDKTokenFreezeParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** The identity to freeze/unfreeze (32 raw bytes). */
    @JvmField var target_identity_id: Pointer? = null
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenClaimParams` in rs-sdk-ffi.h (6 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_claim].
 *
 * C 64-bit layout (cc-verified, size=40):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), distribution_type @28 (4), public_note @32 (8).
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "distribution_type",
    "public_note"
)
class DashSDKTokenClaimParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Distribution type (enum DashSDKTokenDistributionType: 0=PreProgrammed, 1=Perpetual). */
    @JvmField var distribution_type: Int = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenPriceEntry` in rs-sdk-ffi.h — one tier of `SetPrices` pricing.
 * Used as the element type of [DashSDKTokenSetPriceParamsNative.price_entries].
 *
 * C 64-bit layout (cc-verified): amount @0 (8), price @8 (8); size=16.
 */
@Structure.FieldOrder("amount", "price")
class DashSDKTokenPriceEntryNative : Structure() {
    /** Token amount threshold (uint64_t). */
    @JvmField var amount: Long = 0
    /** Price in credits for this amount (uint64_t). */
    @JvmField var price: Long = 0
}

/**
 * Maps to `struct DashSDKTokenSetPriceParams` in rs-sdk-ffi.h (9 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_set_price].
 *
 * C 64-bit layout (cc-verified, size=64):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), pricing_type @28 (4), single_price @32 (8), price_entries @40 (8),
 *   price_entries_count @48 (4), public_note @56 (8).
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "pricing_type",
    "single_price",
    "price_entries",
    "price_entries_count",
    "public_note"
)
class DashSDKTokenSetPriceParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Pricing type (enum DashSDKTokenPricingType: 0=SinglePrice, 1=SetPrices). */
    @JvmField var pricing_type: Int = 0
    /** For SinglePrice — price in credits (ignored for SetPrices) (uint64_t). */
    @JvmField var single_price: Long = 0
    /** For SetPrices — array of [DashSDKTokenPriceEntryNative] (null for SinglePrice). */
    @JvmField var price_entries: Pointer? = null
    /** Number of [price_entries] (uint32_t). */
    @JvmField var price_entries_count: Int = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenPurchaseParams` in rs-sdk-ffi.h (6 fields). Passed **by pointer**
 * to [DashSdkFfi.dash_sdk_token_purchase].
 *
 * C 64-bit layout (cc-verified, size=48):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), amount @32 (8), total_agreed_price @40 (8).
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "amount",
    "total_agreed_price"
)
class DashSDKTokenPurchaseParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** Amount of tokens to purchase (uint64_t). */
    @JvmField var amount: Long = 0
    /** Total agreed price in credits (uint64_t). */
    @JvmField var total_agreed_price: Long = 0
}

/**
 * Maps to `struct DashSDKTokenDestroyFrozenFundsParams` in rs-sdk-ffi.h (6 fields). Passed
 * **by pointer** to [DashSdkFfi.dash_sdk_token_destroy_frozen_funds].
 *
 * C 64-bit layout (cc-verified, size=48):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), frozen_identity_id @32 (8), public_note @40 (8).
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "frozen_identity_id",
    "public_note"
)
class DashSDKTokenDestroyFrozenFundsParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** The frozen identity whose funds to destroy (32 raw bytes). */
    @JvmField var frozen_identity_id: Pointer? = null
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenEmergencyActionParams` in rs-sdk-ffi.h (6 fields). Passed
 * **by pointer** to [DashSdkFfi.dash_sdk_token_emergency_action].
 *
 * C 64-bit layout (cc-verified, size=40):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), action @28 (4), public_note @32 (8).
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "action",
    "public_note"
)
class DashSDKTokenEmergencyActionParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** The emergency action (enum DashSDKTokenEmergencyAction: 0=Pause, 1=Resume). */
    @JvmField var action: Int = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKTokenConfigUpdateParams` in rs-sdk-ffi.h (11 fields). Passed
 * **by pointer** to [DashSdkFfi.dash_sdk_token_update_contract_token_configuration].
 *
 * C 64-bit layout (cc-verified, size=72):
 *   token_contract_id @0 (8), serialized_contract @8 (8), serialized_contract_len @16 (8),
 *   token_position @24 (2), update_type @28 (4), amount @32 (8), bool_value @40 (1),
 *   identity_id @48 (8), group_position @56 (2), action_takers @60 (4), public_note @64 (8).
 * `bool_value` is a C `bool` (1 byte) → JNA [Byte]; the 7-byte gap before [identity_id] is
 * padding JNA inserts automatically from the field order.
 */
@Structure.FieldOrder(
    "token_contract_id",
    "serialized_contract",
    "serialized_contract_len",
    "token_position",
    "update_type",
    "amount",
    "bool_value",
    "identity_id",
    "group_position",
    "action_takers",
    "public_note"
)
class DashSDKTokenConfigUpdateParamsNative : Structure() {
    /** Token contract ID (Base58) — mutually exclusive with [serialized_contract]. */
    @JvmField var token_contract_id: String? = null
    /** Serialized data contract (bincode) — mutually exclusive with [token_contract_id]. */
    @JvmField var serialized_contract: Pointer? = null
    /** Length of [serialized_contract] (uintptr_t). */
    @JvmField var serialized_contract_len: NativeLong = NativeLong(0)
    /** Token position within the contract (uint16_t; defaults to 0). */
    @JvmField var token_position: Short = 0
    /** The configuration update type (enum DashSDKTokenConfigUpdateType). */
    @JvmField var update_type: Int = 0
    /** For MaxSupply updates — the new max supply, 0 for no limit (uint64_t). */
    @JvmField var amount: Long = 0
    /** For boolean updates (e.g. MintingAllowChoosingDestination) — C bool, 1 byte. */
    @JvmField var bool_value: Byte = 0
    /** For identity-based updates — identity ID (32 raw bytes). */
    @JvmField var identity_id: Pointer? = null
    /** For group-based updates — the group position (uint16_t). */
    @JvmField var group_position: Short = 0
    /** For permission updates — the authorized action takers (enum DashSDKAuthorizedActionTakers). */
    @JvmField var action_takers: Int = 0
    /** Optional public note. */
    @JvmField var public_note: String? = null
}

/**
 * Maps to `struct DashSDKDocumentCreateResult` in rs-sdk-ffi.h — the heap payload pointed to
 * by the `data` field of the [DashSDKResultNative] returned from
 * [DashSdkFfi.dash_sdk_document_create].
 *
 * C 64-bit layout (auditor-verified via cc: size=40):
 *   document_handle: DocumentHandle* @0 (8) + entropy: uint8_t[32] @8 (32) = 40 bytes.
 *
 * Construct over the result `data` pointer, [read], copy out [document_handle] + [entropy],
 * then free the backing with [DashSdkFfi.dash_sdk_document_create_result_free]. Do not touch
 * [document_handle] after freeing — the handle ownership transfers to the caller before free
 * (the service reads it out first).
 */
@Structure.FieldOrder("document_handle", "entropy")
class DashSDKDocumentCreateResultNative : Structure {
    /** Handle to the created document. */
    @JvmField var document_handle: Pointer? = null
    /** Entropy used for document-ID generation (exactly 32 bytes). */
    @JvmField var entropy: ByteArray = ByteArray(32)

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
 * Maps to `struct DashSDKNameTimestamp` in rs-sdk-ffi.h — one element of the array pointed at
 * by [DashSDKNameTimestampListNative.entries].
 *
 * C 64-bit layout (auditor-verified via cc offsetof; both fields naturally 8-aligned):
 *   name:     char*    @0 (8)
 *   end_time: uint64_t @8 (8)
 *   = 16 bytes
 *
 * The heap `name` string and the whole array are owned by the parent list and freed by
 * [DashSdkFfi.dash_sdk_name_timestamp_list_free] — do not touch [name] after that.
 */
/**
 * Maps to `struct DpnsRegistrationResult` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_dpns_register_name].
 *
 * C 64-bit layout: three `char*` at offsets 0/8/16 = 24 bytes. The heap strings are owned by
 * the result; release the whole struct with [DashSdkFfi.dash_sdk_dpns_registration_result_free]
 * — do not touch the fields after freeing.
 */
@Structure.FieldOrder("preorder_document_json", "domain_document_json", "full_domain_name")
class DpnsRegistrationResultNative : Structure {
    /** JSON of the preorder document (heap `char*`). */
    @JvmField var preorder_document_json: String? = null
    /** JSON of the domain document (heap `char*`). */
    @JvmField var domain_document_json: String? = null
    /** The full domain name, e.g. "alice.dash" (heap `char*`). */
    @JvmField var full_domain_name: String? = null

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

@Structure.FieldOrder("name", "end_time")
class DashSDKNameTimestampNative : Structure {
    /** The contested name (heap `char*`; owned by the parent list). */
    @JvmField var name: String? = null
    /** End timestamp in milliseconds. */
    @JvmField var end_time: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKNameTimestampList` in rs-sdk-ffi.h — the by-pointer return of
 * [DashSdkFfi.dash_sdk_dpns_get_current_contests].
 *
 * C 64-bit layout (auditor-verified via cc offsetof; both fields naturally 8-aligned):
 *   entries: DashSDKNameTimestamp* @0 (8)
 *   count:   uintptr_t             @8 (8)
 *   = 16 bytes
 *
 * Usage: construct over the returned pointer, [read], iterate [count] contiguous
 * [DashSDKNameTimestampNative] rows off [entries], then release the whole list with
 * [DashSdkFfi.dash_sdk_name_timestamp_list_free] — do not touch any field after freeing.
 */
@Structure.FieldOrder("entries", "count")
class DashSDKNameTimestampListNative : Structure {
    /** Pointer to a contiguous `DashSDKNameTimestamp[count]` array (heap; owned by this list). */
    @JvmField var entries: Pointer? = null
    /** Number of entries in [entries]. */
    @JvmField var count: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKContender` in rs-sdk-ffi.h — one element of the array pointed at by
 * [DashSDKContestInfoNative.contenders].
 *
 * C 64-bit layout: `identity_id: char* @0 (8)` + `vote_count: u32 @8 (4)` + 4 pad = 16 bytes.
 * Owned by the parent contested-names list; freed by [DashSdkFfi.dash_sdk_contested_names_list_free].
 */
@Structure.FieldOrder("identity_id", "vote_count")
class DashSDKContenderNative : Structure {
    /** Base58 identity ID of the contender (heap `char*`; owned by the parent list). */
    @JvmField var identity_id: String? = null
    /** Vote count for this contender. */
    @JvmField var vote_count: Int = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKContestInfo` in rs-sdk-ffi.h — embedded **by value** in
 * [DashSDKContestedNameNative].
 *
 * C 64-bit layout:
 *   contenders:      DashSDKContender* @0  (8)
 *   contender_count: uintptr_t         @8  (8)
 *   abstain_votes:   u32               @16 (4)
 *   lock_votes:      u32               @20 (4)
 *   end_time:        u64               @24 (8)
 *   has_winner:      bool              @32 (1) + 7 pad
 *   = 40 bytes
 *
 * `has_winner` is mapped as [Byte] (read `!= 0`). It is the last field before the struct's
 * 8-byte alignment padding, so a `Boolean` (JNA 4 bytes) would also leave the struct at 40 —
 * but `Byte` matches the C `bool` width exactly.
 */
@Structure.FieldOrder("contenders", "contender_count", "abstain_votes", "lock_votes", "end_time", "has_winner")
class DashSDKContestInfoNative : Structure {
    /** Pointer to a contiguous `DashSDKContender[contender_count]` array (heap; owned by the list). */
    @JvmField var contenders: Pointer? = null
    /** Number of contenders in [contenders]. */
    @JvmField var contender_count: Long = 0
    /** Abstain vote tally (0 if none). */
    @JvmField var abstain_votes: Int = 0
    /** Lock vote tally (0 if none). */
    @JvmField var lock_votes: Int = 0
    /** End time in milliseconds since epoch. */
    @JvmField var end_time: Long = 0
    /** Whether there is a winner (0/1). */
    @JvmField var has_winner: Byte = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKContestedName` in rs-sdk-ffi.h — one element of the array pointed at
 * by [DashSDKContestedNamesListNative.names].
 *
 * C 64-bit layout: `name: char* @0 (8)` + `contest_info: DashSDKContestInfo @8 (40, by value)`
 * = 48 bytes. [contest_info] is an embedded (by-value) struct, read inline on [read].
 */
@Structure.FieldOrder("name", "contest_info")
class DashSDKContestedNameNative : Structure {
    /** The contested name (heap `char*`; owned by the parent list). */
    @JvmField var name: String? = null
    /** Embedded contest state (by value). */
    @JvmField var contest_info: DashSDKContestInfoNative = DashSDKContestInfoNative()

    constructor() : super()
    constructor(p: Pointer) : super(p)
}

/**
 * Maps to `struct DashSDKContestedNamesList` in rs-sdk-ffi.h — the by-pointer return of
 * `dash_sdk_dpns_get_contested_non_resolved_usernames` /
 * `dash_sdk_dpns_get_non_resolved_contests_for_identity`.
 *
 * C 64-bit layout: `names: DashSDKContestedName* @0 (8)` + `count: uintptr_t @8 (8)` = 16 bytes.
 * Read [count] contiguous [DashSDKContestedNameNative] rows off [names], then release the whole
 * hierarchy with [DashSdkFfi.dash_sdk_contested_names_list_free].
 */
@Structure.FieldOrder("names", "count")
class DashSDKContestedNamesListNative : Structure {
    /** Pointer to a contiguous `DashSDKContestedName[count]` array (heap; owned by this list). */
    @JvmField var names: Pointer? = null
    /** Number of names in [names]. */
    @JvmField var count: Long = 0

    constructor() : super()
    constructor(p: Pointer) : super(p)
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

// FFINetwork enum (dash-network.h `typedef enum FFINetwork`). This is a DISTINCT enum
// from DashSDKNetwork above: here Devnet=2 and Regtest=3 (swapped relative to
// DashSDKNetwork), and there is no Local variant. Used by functions whose header types the
// network parameter as `FFINetwork` (e.g. dash_sdk_encode_platform_address).
object FFINetwork {
    const val MAINNET = 0
    const val TESTNET = 1
    const val DEVNET = 2
    const val REGTEST = 3
}
