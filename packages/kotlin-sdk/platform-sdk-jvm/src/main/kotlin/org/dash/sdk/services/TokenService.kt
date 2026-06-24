package org.dash.sdk.services

import com.sun.jna.Memory
import com.sun.jna.Pointer
import java.lang.ref.Reference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSDKTokenBurnParamsNative
import org.dash.sdk.ffi.DashSDKTokenFreezeParamsNative
import org.dash.sdk.ffi.DashSDKTokenMintParamsNative
import org.dash.sdk.ffi.DashSDKTokenTransferParamsNative
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.ffi.toNative
import org.dash.sdk.models.PutSettings
import org.dash.sdk.signing.Signer

/**
 * High-level service for Dash Platform token read operations.
 * Mirrors the SwiftDashSDK token query helpers.
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract.
 *
 * The JSON-returning queries are network calls and run on [Dispatchers.IO].
 * [calculateTokenId] is a process-local derivation (no SDK handle, no network) and is
 * therefore synchronous.
 */
class TokenService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Derive the canonical platform token ID for `(contractId, position)`.
     * Process-local — no network.
     *
     * @param contractId base58-encoded 32-byte data contract ID
     * @param position token contract position within the data contract (C `uint16_t`)
     * @return base58-encoded token ID string
     */
    fun calculateTokenId(contractId: String, position: Int): String =
        ResultUnwrapper.unwrapString(ffi.dash_sdk_calculate_token_id(contractId, position))

    /**
     * Get a token's contract info as a JSON string (contract ID and token position).
     *
     * @param tokenId base58-encoded token ID
     */
    suspend fun getContractInfo(tokenId: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_token_get_contract_info(sdkHandle, tokenId))
    }

    /**
     * Get direct purchase prices as a JSON string (token IDs → pricing info).
     *
     * @param tokenIds comma-separated list of base58-encoded token IDs
     */
    suspend fun getDirectPurchasePrices(tokenIds: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_token_get_direct_purchase_prices(sdkHandle, tokenIds))
    }

    /**
     * Get an identity's token balances as a JSON string (token IDs → balances).
     *
     * @param identityId base58-encoded identity ID
     * @param tokenIds comma-separated list of base58-encoded token IDs
     */
    suspend fun getIdentityBalances(identityId: String, tokenIds: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_token_get_identity_balances(sdkHandle, identityId, tokenIds)
            )
        }

    /**
     * Get an identity's token info as a JSON string (token IDs → info).
     *
     * @param identityId base58-encoded identity ID
     * @param tokenIds comma-separated list of base58-encoded token IDs
     */
    suspend fun getIdentityInfos(identityId: String, tokenIds: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_token_get_identity_infos(sdkHandle, identityId, tokenIds)
            )
        }

    /**
     * Get the last perpetual-distribution claim for an identity on a token, as a JSON string.
     *
     * @param tokenId base58-encoded token ID
     * @param identityId base58-encoded identity ID
     */
    suspend fun getPerpetualDistributionLastClaim(tokenId: String, identityId: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_token_get_perpetual_distribution_last_claim(sdkHandle, tokenId, identityId)
            )
        }

    /**
     * Get pre-programmed distributions for a token as a JSON array string.
     *
     * @param tokenId base58-encoded token ID
     * @param startTimeMs starting time in ms (0 = no start time)
     * @param startRecipient base58-encoded starting recipient ID (null = none)
     * @param startRecipientIncluded whether to include the start recipient
     * @param limit maximum number of distributions to return (0 = default limit)
     */
    suspend fun getPreProgrammedDistributions(
        tokenId: String,
        startTimeMs: Long = 0L,
        startRecipient: String? = null,
        startRecipientIncluded: Boolean = false,
        limit: Int = 0
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_token_get_pre_programmed_distributions(
                sdkHandle, tokenId, startTimeMs, startRecipient, startRecipientIncluded, limit
            )
        )
    }

    /**
     * Get token statuses as a JSON string (token IDs → status info).
     *
     * @param tokenIds comma-separated list of base58-encoded token IDs
     */
    suspend fun getStatuses(tokenIds: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_token_get_statuses(sdkHandle, tokenIds))
    }

    /**
     * Get the total supply of a token as a JSON string.
     *
     * @param tokenId base58-encoded token ID
     */
    suspend fun getTotalSupply(tokenId: String): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_token_get_total_supply(sdkHandle, tokenId))
    }

    // -------------------------------------------------------------------------
    // Token write-path (state transitions) — external-signer pattern. Each is a 1:1 FFI
    // wrapper: build the *Params struct → write() → call → fence → unwrap. The
    // [transitionOwnerId] is 32 raw bytes (packed into a 32-byte Memory); the signing key is
    // an IdentityPublicKeyHandle [Pointer] (from IdentityService.getPublicKeyById /
    // createPublicKey); [signer] supplies the SignerHandle. token_contract_id is the Base58
    // id, so the serialized_contract pair is left null/0. state_transition_creation_options
    // is always NULL. These Rust ops return DashSDKResult::success(null) — no data payload —
    // so they unwrap via ResultUnwrapper.unwrapVoid. Network calls; require a live node.
    // -------------------------------------------------------------------------

    /**
     * Mint [amount] tokens (optionally to [recipientId]) and wait for confirmation.
     *
     * @param transitionOwnerId 32-byte owner identity ID (raw bytes)
     * @param tokenContractId data contract ID (Base58)
     * @param amount amount to mint
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; kept alive across the call
     * @param tokenPosition token position within the contract (default 0)
     * @param recipientId optional 32-byte recipient identity ID (raw bytes; null = mint to owner)
     * @param publicNote optional public note
     * @throws IllegalArgumentException if [transitionOwnerId] (or a non-null [recipientId]) is not 32 bytes
     */
    suspend fun mint(
        transitionOwnerId: ByteArray,
        tokenContractId: String,
        amount: Long,
        signingKeyHandle: Pointer,
        signer: Signer,
        tokenPosition: Int = 0,
        recipientId: ByteArray? = null,
        publicNote: String? = null,
        settings: PutSettings? = null
    ): Unit = withContext(Dispatchers.IO) {
        val ownerMem = transitionOwnerId.toIdMemory("transitionOwnerId")
        val recipientMem = recipientId?.toIdMemory("recipientId")
        val params = DashSDKTokenMintParamsNative().apply {
            token_contract_id = tokenContractId
            token_position = tokenPosition.toShort()
            recipient_id = recipientMem
            this.amount = amount
            public_note = publicNote
            write()
        }
        val ps = settings?.toNative()
        try {
            ResultUnwrapper.unwrapVoid(
                ffi.dash_sdk_token_mint(
                    sdkHandle, ownerMem, params, signingKeyHandle, signer.handle, ps?.pointer, null
                )
            )
        } finally {
            Reference.reachabilityFence(ownerMem)
            Reference.reachabilityFence(recipientMem)
            Reference.reachabilityFence(params)
            Reference.reachabilityFence(signer)
            Reference.reachabilityFence(ps)
        }
    }

    /**
     * Burn [amount] tokens from the owner and wait for confirmation.
     *
     * @param transitionOwnerId 32-byte owner identity ID (raw bytes)
     * @param tokenContractId data contract ID (Base58)
     * @param amount amount to burn
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; kept alive across the call
     * @param tokenPosition token position within the contract (default 0)
     * @param publicNote optional public note
     * @throws IllegalArgumentException if [transitionOwnerId] is not 32 bytes
     */
    suspend fun burn(
        transitionOwnerId: ByteArray,
        tokenContractId: String,
        amount: Long,
        signingKeyHandle: Pointer,
        signer: Signer,
        tokenPosition: Int = 0,
        publicNote: String? = null,
        settings: PutSettings? = null
    ): Unit = withContext(Dispatchers.IO) {
        val ownerMem = transitionOwnerId.toIdMemory("transitionOwnerId")
        val params = DashSDKTokenBurnParamsNative().apply {
            token_contract_id = tokenContractId
            token_position = tokenPosition.toShort()
            this.amount = amount
            public_note = publicNote
            write()
        }
        val ps = settings?.toNative()
        try {
            ResultUnwrapper.unwrapVoid(
                ffi.dash_sdk_token_burn(
                    sdkHandle, ownerMem, params, signingKeyHandle, signer.handle, ps?.pointer, null
                )
            )
        } finally {
            Reference.reachabilityFence(ownerMem)
            Reference.reachabilityFence(params)
            Reference.reachabilityFence(signer)
            Reference.reachabilityFence(ps)
        }
    }

    /**
     * Transfer [amount] tokens to [recipientId] and wait for confirmation.
     *
     * @param transitionOwnerId 32-byte owner identity ID (raw bytes)
     * @param tokenContractId data contract ID (Base58)
     * @param recipientId 32-byte recipient identity ID (raw bytes)
     * @param amount amount to transfer
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; kept alive across the call
     * @param tokenPosition token position within the contract (default 0)
     * @param publicNote optional public note
     * @param privateEncryptedNote optional private encrypted note
     * @param sharedEncryptedNote optional shared encrypted note
     * @throws IllegalArgumentException if [transitionOwnerId] or [recipientId] is not 32 bytes
     */
    suspend fun transfer(
        transitionOwnerId: ByteArray,
        tokenContractId: String,
        recipientId: ByteArray,
        amount: Long,
        signingKeyHandle: Pointer,
        signer: Signer,
        tokenPosition: Int = 0,
        publicNote: String? = null,
        privateEncryptedNote: String? = null,
        sharedEncryptedNote: String? = null,
        settings: PutSettings? = null
    ): Unit = withContext(Dispatchers.IO) {
        val ownerMem = transitionOwnerId.toIdMemory("transitionOwnerId")
        val recipientMem = recipientId.toIdMemory("recipientId")
        val params = DashSDKTokenTransferParamsNative().apply {
            token_contract_id = tokenContractId
            token_position = tokenPosition.toShort()
            recipient_id = recipientMem
            this.amount = amount
            public_note = publicNote
            private_encrypted_note = privateEncryptedNote
            shared_encrypted_note = sharedEncryptedNote
            write()
        }
        val ps = settings?.toNative()
        try {
            ResultUnwrapper.unwrapVoid(
                ffi.dash_sdk_token_transfer(
                    sdkHandle, ownerMem, params, signingKeyHandle, signer.handle, ps?.pointer, null
                )
            )
        } finally {
            Reference.reachabilityFence(ownerMem)
            Reference.reachabilityFence(recipientMem)
            Reference.reachabilityFence(params)
            Reference.reachabilityFence(signer)
            Reference.reachabilityFence(ps)
        }
    }

    /**
     * Freeze a token for [targetIdentityId] and wait for confirmation.
     *
     * @param transitionOwnerId 32-byte owner identity ID (raw bytes)
     * @param tokenContractId data contract ID (Base58)
     * @param targetIdentityId 32-byte identity ID to freeze (raw bytes)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; kept alive across the call
     * @param tokenPosition token position within the contract (default 0)
     * @param publicNote optional public note
     * @throws IllegalArgumentException if [transitionOwnerId] or [targetIdentityId] is not 32 bytes
     */
    suspend fun freeze(
        transitionOwnerId: ByteArray,
        tokenContractId: String,
        targetIdentityId: ByteArray,
        signingKeyHandle: Pointer,
        signer: Signer,
        tokenPosition: Int = 0,
        publicNote: String? = null,
        settings: PutSettings? = null
    ): Unit = withContext(Dispatchers.IO) {
        val params = freezeParams(tokenContractId, targetIdentityId, tokenPosition, publicNote)
        callFreeze(transitionOwnerId, params, signingKeyHandle, signer, settings, unfreeze = false)
    }

    /**
     * Unfreeze a token for [targetIdentityId] and wait for confirmation. Reuses the
     * freeze parameter struct (the FFI shares `DashSDKTokenFreezeParams`).
     *
     * @param transitionOwnerId 32-byte owner identity ID (raw bytes)
     * @param tokenContractId data contract ID (Base58)
     * @param targetIdentityId 32-byte identity ID to unfreeze (raw bytes)
     * @param signingKeyHandle IdentityPublicKeyHandle that signs the transition
     * @param signer external signer; kept alive across the call
     * @param tokenPosition token position within the contract (default 0)
     * @param publicNote optional public note
     * @throws IllegalArgumentException if [transitionOwnerId] or [targetIdentityId] is not 32 bytes
     */
    suspend fun unfreeze(
        transitionOwnerId: ByteArray,
        tokenContractId: String,
        targetIdentityId: ByteArray,
        signingKeyHandle: Pointer,
        signer: Signer,
        tokenPosition: Int = 0,
        publicNote: String? = null,
        settings: PutSettings? = null
    ): Unit = withContext(Dispatchers.IO) {
        val params = freezeParams(tokenContractId, targetIdentityId, tokenPosition, publicNote)
        callFreeze(transitionOwnerId, params, signingKeyHandle, signer, settings, unfreeze = true)
    }

    // ---- write-path helpers ----

    /** Build a written [DashSDKTokenFreezeParamsNative] (shared by freeze + unfreeze). */
    private fun freezeParams(
        tokenContractId: String,
        targetIdentityId: ByteArray,
        tokenPosition: Int,
        publicNote: String?
    ): DashSDKTokenFreezeParamsNative {
        val targetMem = targetIdentityId.toIdMemory("targetIdentityId")
        return DashSDKTokenFreezeParamsNative().apply {
            token_contract_id = tokenContractId
            token_position = tokenPosition.toShort()
            target_identity_id = targetMem
            public_note = publicNote
            write()
        }
    }

    /** Call freeze or unfreeze with a prepared params struct, fencing all native inputs. */
    private fun callFreeze(
        transitionOwnerId: ByteArray,
        params: DashSDKTokenFreezeParamsNative,
        signingKeyHandle: Pointer,
        signer: Signer,
        settings: PutSettings?,
        unfreeze: Boolean
    ) {
        val ownerMem = transitionOwnerId.toIdMemory("transitionOwnerId")
        val ps = settings?.toNative()
        try {
            val result = if (unfreeze) {
                ffi.dash_sdk_token_unfreeze(
                    sdkHandle, ownerMem, params, signingKeyHandle, signer.handle, ps?.pointer, null
                )
            } else {
                ffi.dash_sdk_token_freeze(
                    sdkHandle, ownerMem, params, signingKeyHandle, signer.handle, ps?.pointer, null
                )
            }
            ResultUnwrapper.unwrapVoid(result)
        } finally {
            Reference.reachabilityFence(ownerMem)
            Reference.reachabilityFence(params)
            Reference.reachabilityFence(signer)
            Reference.reachabilityFence(ps)
        }
    }

    /** Pack a required 32-byte raw identity ID into a 32-byte [Memory]. */
    private fun ByteArray.toIdMemory(name: String): Memory {
        require(size == 32) { "$name must be 32 bytes, was $size" }
        return Memory(32).apply { write(0, this@toIdMemory, 0, 32) }
    }
}
