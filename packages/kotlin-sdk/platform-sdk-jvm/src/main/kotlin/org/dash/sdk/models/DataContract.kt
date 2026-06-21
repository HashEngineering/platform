package org.dash.sdk.models

/**
 * Represents a Dash Platform data contract.
 */
data class DataContract(
    val id: String,
    val json: String
)

/**
 * Result of a data-contract fetch that optionally returns the JSON representation and/or
 * the serialized bytes (see [org.dash.sdk.services.DataContractService.fetchWithSerialization]).
 *
 * @property id the contract ID that was requested (hex)
 * @property json the JSON representation, or null if not requested / not returned
 * @property serialized the serialized contract bytes, or null if not requested / not returned
 */
data class DataContractFetchResult(
    val id: String,
    val json: String?,
    val serialized: ByteArray?
)
