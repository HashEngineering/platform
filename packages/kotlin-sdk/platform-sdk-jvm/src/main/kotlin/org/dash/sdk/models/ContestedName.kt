package org.dash.sdk.models

/**
 * A current DPNS contest: a contested name and the end timestamp of its vote poll.
 *
 * Returned by [org.dash.sdk.services.DpnsService.getCurrentContests], read out of the
 * native `DashSDKNameTimestampList` before it is freed.
 */
data class ContestedName(
    /** The contested DPNS name. */
    val name: String,
    /** End timestamp of the contest's vote poll, in milliseconds. */
    val endTime: Long,
)
