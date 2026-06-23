package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

/**
 * Read-path system, status, and protocol-version queries.
 * Mirrors the SwiftDashSDK `SDKStatus` / system query helpers.
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract.
 *
 * The JSON-returning queries are network calls and run on [Dispatchers.IO]; [version]
 * is a process-local call and is therefore synchronous.
 */
class SystemService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /** SDK version string (e.g. "2.0.0"). Local call — no network. */
    fun version(): String = ffi.dash_sdk_version().orEmpty()

    /** Overall SDK status as a JSON string. */
    suspend fun status(): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_get_status(sdkHandle))
    }

    /** Platform-only status as a JSON string. */
    suspend fun platformStatus(): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_get_platform_status(sdkHandle))
    }

    /**
     * Epoch info for [count] epochs starting at [startEpoch] (null = current),
     * ordered by [ascending]. Returns the raw JSON string from the platform.
     */
    suspend fun epochsInfo(
        startEpoch: String? = null,
        count: Int = 1,
        ascending: Boolean = true
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_system_get_epochs_info(sdkHandle, startEpoch, count, ascending)
        )
    }

    /** Current quorums info as a JSON string. */
    suspend fun currentQuorumsInfo(): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_system_get_current_quorums_info(sdkHandle))
    }

    /** Protocol-version upgrade state as a JSON string. */
    suspend fun protocolUpgradeState(): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(ffi.dash_sdk_protocol_version_get_upgrade_state(sdkHandle))
    }

    /**
     * Protocol-version upgrade vote status from [startProTxHash] over [count]
     * masternodes, as a JSON string.
     */
    suspend fun protocolUpgradeVoteStatus(
        startProTxHash: String,
        count: Int
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_protocol_version_get_upgrade_vote_status(sdkHandle, startProTxHash, count)
        )
    }

    /**
     * Fetch raw GroveDB path elements for a low-level state query.
     *
     * @param pathJson JSON array of the GroveDB path segments
     * @param keysJson JSON array of the keys to read at that path
     * @return JSON array string of the elements (or null if not found)
     */
    suspend fun pathElements(pathJson: String, keysJson: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_system_get_path_elements(sdkHandle, pathJson, keysJson)
            )
        }
}
