package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

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
}
