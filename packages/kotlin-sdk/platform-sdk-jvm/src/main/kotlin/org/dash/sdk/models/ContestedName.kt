package org.dash.sdk.models

/**
 * A contested DPNS name with its full contest state (contenders + vote tallies).
 *
 * Returned by [org.dash.sdk.services.DpnsService.getContestedNonResolvedUsernames] and
 * [org.dash.sdk.services.DpnsService.getNonResolvedContestsForIdentity], read out of the
 * native `DashSDKContestedNamesList` before it is freed.
 */
data class ContestedName(
    /** The contested DPNS name. */
    val name: String,
    /** Contest state: contenders, abstain/lock tallies, end time, winner flag. */
    val contestInfo: ContestInfo,
)

/**
 * Contest state for a contested DPNS name.
 */
data class ContestInfo(
    /** The identities contending for the name, with their vote counts. */
    val contenders: List<Contender>,
    /** Abstain vote tally (0 if none). */
    val abstainVotes: Int,
    /** Lock vote tally (0 if none). */
    val lockVotes: Int,
    /** End time of the contest's vote poll, in milliseconds since epoch. */
    val endTime: Long,
    /** Whether the contest has a winner. */
    val hasWinner: Boolean,
)

/**
 * One contender in a contested DPNS name.
 */
data class Contender(
    /** Base58 identity ID of the contender. */
    val identityId: String,
    /** Vote count for this contender. */
    val voteCount: Int,
)
