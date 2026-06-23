package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

/**
 * High-level service for evonode read queries (proposed epoch blocks).
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract. All queries
 * are network calls and run on [Dispatchers.IO]; each returns a JSON string result.
 */
class EvonodeService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Get proposed epoch blocks for a set of evonode IDs, as a JSON string.
     *
     * @param epoch epoch index (C uint32)
     * @param idsJson JSON array of evonode (proTxHash) IDs
     */
    suspend fun getProposedEpochBlocksByIds(epoch: Int, idsJson: String): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_evonode_get_proposed_epoch_blocks_by_ids(sdkHandle, epoch, idsJson)
            )
        }

    /**
     * Get proposed epoch blocks over a range, as a JSON string.
     *
     * @param epoch epoch index (C uint32)
     * @param limit max results (C uint32; 0 = server default)
     * @param startAfter optional cursor (proTxHash to page after); null for the first page
     * @param startAt optional cursor (proTxHash to page from, inclusive); null for none
     */
    suspend fun getProposedEpochBlocksByRange(
        epoch: Int,
        limit: Int = 0,
        startAfter: String? = null,
        startAt: String? = null
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_evonode_get_proposed_epoch_blocks_by_range(
                sdkHandle, epoch, limit, startAfter, startAt
            )
        )
    }
}
