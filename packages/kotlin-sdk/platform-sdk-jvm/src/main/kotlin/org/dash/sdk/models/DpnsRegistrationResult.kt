package org.dash.sdk.models

/**
 * Result of a DPNS username registration: the two created documents and the full domain name.
 *
 * Returned by [org.dash.sdk.services.DpnsService.registerName], read out of the native
 * `DpnsRegistrationResult` before it is freed.
 */
data class DpnsRegistrationResult(
    /** JSON of the created preorder document. */
    val preorderDocumentJson: String,
    /** JSON of the created domain document. */
    val domainDocumentJson: String,
    /** The full registered domain name, e.g. "alice.dash". */
    val fullDomainName: String,
)
