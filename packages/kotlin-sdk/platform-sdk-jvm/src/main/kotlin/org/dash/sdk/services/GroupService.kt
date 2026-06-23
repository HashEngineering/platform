package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

/**
 * High-level service for group read queries (multi-party group actions on a data contract).
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract. All queries
 * are network calls and run on [Dispatchers.IO]; each returns a JSON string result.
 */
class GroupService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Get info for the group at [groupContractPosition] in a contract, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param groupContractPosition group position within the contract (C uint16, unsigned 16-bit)
     */
    suspend fun getInfo(contractId: String, groupContractPosition: Int): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_group_get_info(sdkHandle, contractId, groupContractPosition)
            )
        }

    /**
     * Get infos for all groups in a contract, as a JSON string.
     *
     * @param startAtPosition optional cursor (group position to page from); null for the first page
     * @param limit max results (C uint32; 0 = server default)
     */
    suspend fun getInfos(startAtPosition: String? = null, limit: Int = 0): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_group_get_infos(sdkHandle, startAtPosition, limit)
            )
        }

    /**
     * Get group actions, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param groupContractPosition group position within the contract (C uint16, unsigned 16-bit)
     * @param status action status filter (C uint8)
     * @param startAtActionId optional cursor (action ID to page after); null for the first page
     * @param limit max results (C uint16, unsigned 16-bit; 0 = server default)
     */
    suspend fun getActions(
        contractId: String,
        groupContractPosition: Int,
        status: Int,
        startAtActionId: String? = null,
        limit: Int = 0
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_group_get_actions(
                sdkHandle, contractId, groupContractPosition, status.toByte(), startAtActionId, limit
            )
        )
    }

    /**
     * Get the signers of a group action, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param groupContractPosition group position within the contract (C uint16, unsigned 16-bit)
     * @param status action status filter (C uint8)
     * @param actionId base58-encoded group action ID
     */
    suspend fun getActionSigners(
        contractId: String,
        groupContractPosition: Int,
        status: Int,
        actionId: String
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_group_get_action_signers(
                sdkHandle, contractId, groupContractPosition, status.toByte(), actionId
            )
        )
    }
}
