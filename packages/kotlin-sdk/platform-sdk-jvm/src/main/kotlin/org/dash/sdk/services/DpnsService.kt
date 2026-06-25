package org.dash.sdk.services

import com.sun.jna.Pointer
import java.lang.ref.Reference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.dash.sdk.ffi.DashSDKContenderNative
import org.dash.sdk.ffi.DashSDKContestedNameNative
import org.dash.sdk.ffi.DashSDKContestedNamesListNative
import org.dash.sdk.ffi.DashSDKNameTimestampListNative
import org.dash.sdk.ffi.DashSDKNameTimestampNative
import org.dash.sdk.ffi.DpnsRegistrationResultNative
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.ContestInfo
import org.dash.sdk.models.ContestedName
import org.dash.sdk.models.Contender
import org.dash.sdk.models.CurrentContest
import org.dash.sdk.models.DpnsRegistrationResult
import org.dash.sdk.signing.Signer

/**
 * High-level service for Dash Platform Naming Service (DPNS) operations.
 * Mirrors SwiftDashSDK/DPP/DPPIdentity.swift name resolution helpers.
 *
 * Every method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out). Network queries are `suspend` on [Dispatchers.IO]; the validation helpers
 * ([getValidationMessage], [normalizeUsername], [isValidUsername], [isContestedUsername])
 * are process-local (no SDK handle, no network) and synchronous.
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
     * Search for DPNS names by prefix ("starts with").
     *
     * @param prefix name prefix to search
     * @param limit max results (0 = server default)
     * @return list of matching fully-qualified names (e.g. "alice.dash")
     */
    suspend fun search(prefix: String, limit: Int = 10): List<String> = withContext(Dispatchers.IO) {
        runCatching {
            val result = ffi.dash_sdk_dpns_search(sdkHandle, prefix, limit)
            parseSearchResults(ResultUnwrapper.unwrapString(result))
        }.getOrDefault(emptyList())
    }

    /**
     * Get all contested DPNS usernames as a JSON string.
     *
     * @param limit max results (C uint32; 0 = server default)
     * @param startAfter optional cursor (label to page after); null for the first page
     */
    suspend fun getAllContestedUsernames(limit: Int = 0, startAfter: String? = null): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_dpns_get_all_contested_usernames(sdkHandle, limit, startAfter)
            )
        }

    /**
     * Get all contested DPNS usernames where [identityId] is a contender, as a JSON string.
     *
     * @param identityId base58-encoded identity ID
     * @param limit max results (C uint32; 0 = server default)
     */
    suspend fun getContestedUsernamesByIdentity(identityId: String, limit: Int = 0): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_dpns_get_contested_usernames_by_identity(sdkHandle, identityId, limit)
            )
        }

    /**
     * Get the vote state for a contested DPNS username [label], as a JSON string.
     *
     * @param label contested label (without ".dash" suffix)
     * @param limit max results (C uint32; 0 = server default)
     */
    suspend fun getContestedVoteState(label: String, limit: Int = 0): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_dpns_get_contested_vote_state(sdkHandle, label, limit)
            )
        }

    /**
     * Get the contested DPNS usernames [identityId] has voted on, as a JSON string.
     *
     * @param identityId base58-encoded identity ID
     * @param limit max results (C uint32; 0 = server default)
     * @param offset pagination offset (C uint16, unsigned 16-bit)
     */
    suspend fun getIdentityVotes(identityId: String, limit: Int = 0, offset: Int = 0): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_dpns_get_identity_votes(sdkHandle, identityId, limit, offset)
            )
        }

    /**
     * Get the DPNS usernames owned by [identityId], as a JSON string.
     *
     * @param identityId base58-encoded identity ID
     * @param limit max results (C uint32; 0 = server default)
     */
    suspend fun getUsernames(identityId: String, limit: Int = 0): String =
        withContext(Dispatchers.IO) {
            ResultUnwrapper.unwrapString(
                ffi.dash_sdk_dpns_get_usernames(sdkHandle, identityId, limit)
            )
        }

    /**
     * Get a human-readable validation message for a DPNS [name]. Process-local — no network.
     *
     * @param name DPNS label or fully-qualified name to validate
     */
    fun getValidationMessage(name: String): String =
        ResultUnwrapper.unwrapString(ffi.dash_sdk_dpns_get_validation_message(name))

    /**
     * Normalize a DPNS [name] (case-folding / homoglyph normalization). Process-local — no network.
     *
     * @param name DPNS label to normalize
     */
    fun normalizeUsername(name: String): String =
        ResultUnwrapper.unwrapString(ffi.dash_sdk_dpns_normalize_username(name))

    /**
     * Whether [name] is a valid DPNS username. Process-local — no network.
     *
     * Backed by an FFI function returning a raw `int32_t` (-1 error, 0 invalid, 1 valid),
     * so this never throws — returns true only for an explicit `1`.
     */
    fun isValidUsername(name: String): Boolean =
        ffi.dash_sdk_dpns_is_valid_username(name) == 1

    /**
     * Whether [name] is a contested DPNS username. Process-local — no network.
     *
     * Backed by an FFI function returning a raw `int32_t` (-1 error, 0 not-contested,
     * 1 contested), so this never throws — returns true only for an explicit `1`.
     */
    fun isContestedUsername(name: String): Boolean =
        ffi.dash_sdk_dpns_is_contested_username(name) == 1

    /**
     * Get current DPNS contests (active vote polls) ending within [startTime]..[endTime] (ms).
     *
     * Backed by a function returning a native `DashSDKNameTimestampList *`; this reads the
     * entries into a [List] of [CurrentContest] and frees the native list.
     *
     * @param startTime range start, ms since epoch
     * @param endTime range end, ms since epoch
     * @param limit max results (C uint16, unsigned 16-bit; 0 = server default)
     */
    suspend fun getCurrentContests(startTime: Long, endTime: Long, limit: Int = 0): List<CurrentContest> =
        withContext(Dispatchers.IO) {
            val listPtr = ffi.dash_sdk_dpns_get_current_contests(sdkHandle, startTime, endTime, limit)
                ?: return@withContext emptyList()
            try {
                val list = DashSDKNameTimestampListNative(listPtr).apply { read() }
                val count = list.count.toInt()
                val entriesPtr = list.entries
                if (count <= 0 || entriesPtr == null) {
                    emptyList()
                } else {
                    val first = DashSDKNameTimestampNative(entriesPtr).apply { read() }
                    @Suppress("UNCHECKED_CAST")
                    val rows = first.toArray(count) as Array<DashSDKNameTimestampNative>
                    rows.map { CurrentContest(name = it.name ?: "", endTime = it.end_time) }
                }
            } finally {
                ffi.dash_sdk_name_timestamp_list_free(listPtr)
            }
        }

    /**
     * All contested DPNS usernames that have not yet resolved, with full contest state
     * (contenders + vote tallies). Backed by a function returning a native
     * `DashSDKContestedNamesList *`; reads the entries into a [List] of [ContestedName] and
     * frees the native list.
     *
     * @param limit max results (0 = server default)
     */
    suspend fun getContestedNonResolvedUsernames(limit: Int = 0): List<ContestedName> =
        withContext(Dispatchers.IO) {
            readContestedNamesList(
                ffi.dash_sdk_dpns_get_contested_non_resolved_usernames(sdkHandle, limit)
            )
        }

    /**
     * Unresolved contested usernames an identity is a contender in, with full contest state.
     * Same shape as [getContestedNonResolvedUsernames].
     *
     * @param identityId base58 identity ID
     * @param limit max results (0 = server default)
     */
    suspend fun getNonResolvedContestsForIdentity(identityId: String, limit: Int = 0): List<ContestedName> =
        withContext(Dispatchers.IO) {
            readContestedNamesList(
                ffi.dash_sdk_dpns_get_non_resolved_contests_for_identity(sdkHandle, identityId, limit)
            )
        }

    /**
     * Read a native `DashSDKContestedNamesList *` ([listPtr]) into Kotlin models and free it.
     * Each element embeds a [DashSDKContestInfoNative] by value, which in turn points at a
     * contiguous `DashSDKContender[]`. Returns empty for a null/empty list.
     */
    private fun readContestedNamesList(listPtr: Pointer?): List<ContestedName> {
        if (listPtr == null) return emptyList()
        try {
            val list = DashSDKContestedNamesListNative(listPtr).apply { read() }
            val count = list.count.toInt()
            val namesPtr = list.names
            if (count <= 0 || namesPtr == null) return emptyList()
            @Suppress("UNCHECKED_CAST")
            val rows = (DashSDKContestedNameNative(namesPtr).apply { read() }
                .toArray(count) as Array<DashSDKContestedNameNative>)
            return rows.map { row ->
                val info = row.contest_info
                val contenders = readContenders(info.contenders, info.contender_count.toInt())
                ContestedName(
                    name = row.name ?: "",
                    contestInfo = ContestInfo(
                        contenders = contenders,
                        abstainVotes = info.abstain_votes,
                        lockVotes = info.lock_votes,
                        endTime = info.end_time,
                        hasWinner = info.has_winner.toInt() != 0,
                    )
                )
            }
        } finally {
            ffi.dash_sdk_contested_names_list_free(listPtr)
        }
    }

    private fun readContenders(contendersPtr: Pointer?, contenderCount: Int): List<Contender> {
        if (contenderCount <= 0 || contendersPtr == null) return emptyList()
        @Suppress("UNCHECKED_CAST")
        val rows = (DashSDKContenderNative(contendersPtr).apply { read() }
            .toArray(contenderCount) as Array<DashSDKContenderNative>)
        return rows.map { Contender(identityId = it.identity_id ?: "", voteCount = it.vote_count) }
    }

    /**
     * Register a DPNS username (preorder + domain) in one operation. Network call — requires
     * a live node. The SDK generates entropy and submits both documents in order.
     *
     * @param label the username label, e.g. "alice"
     * @param identityHandle handle for the registering identity (e.g. from
     *   [IdentityService.fetchHandle] / [IdentityService.createFromComponents])
     * @param signingKeyHandle an `IdentityPublicKeyHandle` for the signing key (e.g. from
     *   [IdentityService.getPublicKeyById])
     * @param signer signs the preorder/domain documents; keep alive across the call
     * @return the created documents + full domain name
     * @throws org.dash.sdk.models.DashSDKException on failure
     */
    suspend fun registerName(
        label: String,
        identityHandle: Pointer,
        signingKeyHandle: Pointer,
        signer: Signer,
    ): DpnsRegistrationResult = withContext(Dispatchers.IO) {
        val resultPtr = ResultUnwrapper.unwrapHandle(
            ffi.dash_sdk_dpns_register_name(sdkHandle, label, identityHandle, signingKeyHandle, signer.handle)
        )
        Reference.reachabilityFence(signer)
        try {
            val r = DpnsRegistrationResultNative(resultPtr).apply { read() }
            DpnsRegistrationResult(
                preorderDocumentJson = r.preorder_document_json ?: "",
                domainDocumentJson = r.domain_document_json ?: "",
                fullDomainName = r.full_domain_name ?: "",
            )
        } finally {
            ffi.dash_sdk_dpns_registration_result_free(resultPtr)
        }
    }

    companion object {
        /**
         * Parse the JSON returned by `dash_sdk_dpns_search` into a list of fully-qualified
         * names. The native function returns an array of objects
         * (`[{"label":"alice","fullName":"alice.dash","ownerId":"..."}]`); we extract each
         * object's `fullName` (falling back to `label`). For robustness, a plain JSON string
         * array (`["alice.dash", ...]`) is also accepted. Never throws — returns an empty
         * list for empty/blank/unrecognized input.
         */
        internal fun parseSearchResults(json: String): List<String> {
            val trimmed = json.trim()
            if (trimmed.isEmpty() || trimmed == "[]") return emptyList()

            val objects = splitJsonObjects(trimmed)
            if (objects.isNotEmpty()) {
                return objects.mapNotNull { obj ->
                    (jsonField(obj, "fullName") ?: jsonField(obj, "label"))?.takeIf { it.isNotBlank() }
                }
            }
            // Fallback: a plain array of quoted strings.
            return Regex(""""([^"\\]*(?:\\.[^"\\]*)*)"""").findAll(trimmed)
                .map { it.groupValues[1] }
                .filter { it.isNotBlank() }
                .toList()
        }

        /** Extract a string value for [key] from a single JSON object literal. */
        private fun jsonField(obj: String, key: String): String? =
            Regex(""""$key"\s*:\s*"([^"\\]*(?:\\.[^"\\]*)*)"""").find(obj)?.groupValues?.get(1)

        /** Split a JSON array into its top-level `{...}` object substrings (brace-depth scan). */
        private fun splitJsonObjects(json: String): List<String> {
            val objects = mutableListOf<String>()
            var depth = 0
            var start = -1
            for (i in json.indices) {
                when (json[i]) {
                    '{' -> if (depth++ == 0) start = i
                    '}' -> if (--depth == 0 && start >= 0) {
                        objects += json.substring(start, i + 1)
                        start = -1
                    }
                }
            }
            return objects
        }
    }
}
