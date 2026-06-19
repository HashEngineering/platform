package org.dash.sdk.models

/**
 * Represents a Dash Platform identity.
 */
data class Identity(
    val id: String,
    val balance: Long,
    val revision: Long,
    val publicKeysCount: Int
)
