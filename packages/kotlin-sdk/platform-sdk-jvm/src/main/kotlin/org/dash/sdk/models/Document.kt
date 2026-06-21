package org.dash.sdk.models

/**
 * Represents a Dash Platform document.
 */
data class Document(
    val id: String,
    val ownerId: String,
    val dataContractId: String,
    val documentType: String,
    val revision: Long,
    val createdAt: Long,
    val updatedAt: Long,
    /** Raw JSON properties of the document. */
    val propertiesJson: String
)
