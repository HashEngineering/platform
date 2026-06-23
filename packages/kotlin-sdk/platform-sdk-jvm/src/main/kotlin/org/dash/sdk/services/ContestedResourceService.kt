package org.dash.sdk.services

import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper

/**
 * High-level service for contested-resource and voting read queries.
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract. All queries
 * are network calls and run on [Dispatchers.IO]; each returns a JSON string result.
 */
class ContestedResourceService internal constructor(private val sdkHandle: Pointer) {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Get the contested-resource votes cast by [identityId], as a JSON string.
     *
     * @param identityId base58-encoded identity ID
     * @param limit max results (C uint32; 0 = server default)
     * @param offset pagination offset (C uint32)
     * @param orderAscending order results ascending by resource
     */
    suspend fun getIdentityVotes(
        identityId: String,
        limit: Int = 0,
        offset: Int = 0,
        orderAscending: Boolean = true
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_contested_resource_get_identity_votes(
                sdkHandle, identityId, limit, offset, orderAscending
            )
        )
    }

    /**
     * Get contested resources for an index, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param documentTypeName document type within the contract
     * @param indexName contested index name
     * @param startIndexValuesJson JSON array of start index values (cursor)
     * @param endIndexValuesJson JSON array of end index values (cursor)
     * @param count max results (C uint32; 0 = server default)
     * @param orderAscending order results ascending
     */
    suspend fun getResources(
        contractId: String,
        documentTypeName: String,
        indexName: String,
        startIndexValuesJson: String,
        endIndexValuesJson: String,
        count: Int = 0,
        orderAscending: Boolean = true
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_contested_resource_get_resources(
                sdkHandle, contractId, documentTypeName, indexName,
                startIndexValuesJson, endIndexValuesJson, count, orderAscending
            )
        )
    }

    /**
     * Get the vote state for a contested resource, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param documentTypeName document type within the contract
     * @param indexName contested index name
     * @param indexValuesJson JSON array of index values identifying the contest
     * @param resultType result type (C uint8; e.g. 0 = documents, 1 = vote tally)
     * @param allowIncludeLockedAndAbstainingVoteTally include lock/abstain tallies
     * @param count max results (C uint32; 0 = server default)
     */
    suspend fun getVoteState(
        contractId: String,
        documentTypeName: String,
        indexName: String,
        indexValuesJson: String,
        resultType: Int = 0,
        allowIncludeLockedAndAbstainingVoteTally: Boolean = false,
        count: Int = 0
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_contested_resource_get_vote_state(
                sdkHandle, contractId, documentTypeName, indexName, indexValuesJson,
                resultType.toByte(), allowIncludeLockedAndAbstainingVoteTally, count
            )
        )
    }

    /**
     * Get the voters for [contestantId] on a contested resource, as a JSON string.
     *
     * @param contractId base58-encoded data contract ID
     * @param documentTypeName document type within the contract
     * @param indexName contested index name
     * @param indexValuesJson JSON array of index values identifying the contest
     * @param contestantId base58-encoded contestant identity ID
     * @param count max results (C uint32; 0 = server default)
     * @param orderAscending order results ascending
     */
    suspend fun getVotersForIdentity(
        contractId: String,
        documentTypeName: String,
        indexName: String,
        indexValuesJson: String,
        contestantId: String,
        count: Int = 0,
        orderAscending: Boolean = true
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_contested_resource_get_voters_for_identity(
                sdkHandle, contractId, documentTypeName, indexName, indexValuesJson,
                contestantId, count, orderAscending
            )
        )
    }

    /**
     * Get vote polls by end date, as a JSON string.
     *
     * @param startTimeMs range start, ms since epoch (C uint64)
     * @param startTimeIncluded include the start boundary
     * @param endTimeMs range end, ms since epoch (C uint64)
     * @param endTimeIncluded include the end boundary
     * @param limit max results (C uint32; 0 = server default)
     * @param offset pagination offset (C uint32)
     * @param ascending order results ascending by end date
     */
    suspend fun getVotePollsByEndDate(
        startTimeMs: Long,
        startTimeIncluded: Boolean = true,
        endTimeMs: Long,
        endTimeIncluded: Boolean = true,
        limit: Int = 0,
        offset: Int = 0,
        ascending: Boolean = true
    ): String = withContext(Dispatchers.IO) {
        ResultUnwrapper.unwrapString(
            ffi.dash_sdk_voting_get_vote_polls_by_end_date(
                sdkHandle, startTimeMs, startTimeIncluded, endTimeMs, endTimeIncluded,
                limit, offset, ascending
            )
        )
    }
}
