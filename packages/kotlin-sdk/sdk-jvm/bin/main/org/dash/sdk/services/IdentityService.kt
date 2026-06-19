package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.Identity

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
        val result = ffi.dash_sdk_identity_fetch(sdkHandle, identityId)
        val identityHandle = ResultUnwrapper.unwrapHandle(result)
        try {
            parseIdentityInfo(identityHandle)
        } finally {
            ffi.dash_sdk_identity_handle_free(identityHandle)
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

    private fun parseIdentityInfo(identityHandle: Pointer): Identity {
        val infoResult = ffi.dash_sdk_identity_get_info(identityHandle)
        val infoJson = ResultUnwrapper.unwrapString(infoResult)

        // Parse simple JSON fields (avoids pulling in a full JSON library dependency)
        return Identity(
            id = jsonField(infoJson, "id"),
            balance = jsonLong(infoJson, "balance"),
            revision = jsonLong(infoJson, "revision"),
            publicKeysCount = jsonInt(infoJson, "public_keys_count")
        )
    }

    // Minimal JSON field extractors for the flat identity info structure
    private fun jsonField(json: String, key: String): String =
        Regex(""""$key"\s*:\s*"([^"]+)"""").find(json)?.groupValues?.get(1) ?: ""

    private fun jsonLong(json: String, key: String): Long =
        Regex(""""$key"\s*:\s*(\d+)""").find(json)?.groupValues?.get(1)?.toLongOrNull() ?: 0L

    private fun jsonInt(json: String, key: String): Int =
        Regex(""""$key"\s*:\s*(\d+)""").find(json)?.groupValues?.get(1)?.toIntOrNull() ?: 0
}
