package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

/**
 * High-level service for Dash Platform Naming Service (DPNS) operations.
 * Mirrors SwiftDashSDK/DPP/DPPIdentity.swift name resolution helpers.
 */
class DpnsService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Resolve a DPNS name (e.g. "alice.dash") to the owner's identity ID.
     *
     * @param name fully-qualified DPNS name
     * @return identity ID hex string, or null if not found
     */
    suspend fun resolve(name: String): String? = withContext(Dispatchers.IO) {
        runCatching {
            val result = ffi.dash_sdk_dpns_resolve(sdkHandle, name)
            ResultUnwrapper.unwrapString(result).takeIf { it.isNotBlank() }
        }.getOrNull()
    }

    /**
     * Check whether a DPNS name is available for registration.
     *
     * @param name label (without ".dash" suffix), e.g. "alice"
     * @return true if available
     */
    suspend fun checkAvailability(name: String): Boolean = withContext(Dispatchers.IO) {
        runCatching {
            val result = ffi.dash_sdk_dpns_check_availability(sdkHandle, name)
            ResultUnwrapper.unwrapString(result) == "true"
        }.getOrDefault(false)
    }

    /**
     * Search for DPNS names by prefix.
     *
     * @param prefix name prefix to search
     * @param limit max results (0 = server default)
     * @return list of matching names
     */
    suspend fun search(prefix: String, limit: Int = 10): List<String> = withContext(Dispatchers.IO) {
        runCatching {
            val result = ffi.dash_sdk_dpns_search(sdkHandle, prefix, limit)
            val json = ResultUnwrapper.unwrapString(result)
            // Parse JSON string array ["alice.dash","alice2.dash",...]
            Regex(""""([^"]+)"""").findAll(json).map { it.groupValues[1] }.toList()
        }.getOrDefault(emptyList())
    }
}
