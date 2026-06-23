package org.dash.sdk.models

/**
 * Result of a credit transfer between identities: the sender's and receiver's final
 * balances (in credits) after the transfer.
 */
data class TransferCreditsResult(
    val senderBalance: Long,
    val receiverBalance: Long,
)
