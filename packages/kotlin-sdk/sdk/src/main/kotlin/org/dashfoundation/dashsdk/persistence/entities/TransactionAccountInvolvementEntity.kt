package org.dashfoundation.dashsdk.persistence.entities

import androidx.room.Entity
import androidx.room.ForeignKey
import androidx.room.Index

/**
 * Explicit transaction↔typed-account membership, carrying that account's
 * slice of the transaction.
 *
 * Funds transactions also have TXO-derived membership, but provider special
 * transactions may match only through their payload and create no TXO. The
 * account FK points at the row carrying the complete typed account identity.
 *
 * **Why the per-account slice lives here.** Upstream emits ONE record per
 * matched account, each carrying only that account's [netAmount]; a
 * transaction spending a CoinJoin coin with BIP44 change produces two. The
 * transactions table holds one row per txid, so the last slice to arrive used
 * to overwrite the previous one and the stored net became a fragment (field
 * case: a 10.0001 DASH send stored as -0.00100227). Slices only combined when
 * they happened to land in the same persistence batch, which a rescan does not
 * guarantee — the cross-batch fold gap.
 *
 * Keying the slice on `(transactionTxid, accountId)` makes re-delivery a
 * REPLACE rather than an add, so the transaction's net can be derived as the
 * sum over its involvement rows and is correct however the slices are
 * scheduled, and idempotent under rescans and corrective callbacks.
 */
@Entity(
    tableName = "transaction_account_involvements",
    primaryKeys = ["transactionTxid", "accountId"],
    indices = [Index(value = ["accountId"])],
    foreignKeys = [
        ForeignKey(
            entity = TransactionEntity::class,
            parentColumns = ["txid"],
            childColumns = ["transactionTxid"],
            onDelete = ForeignKey.CASCADE,
        ),
        ForeignKey(
            entity = AccountEntity::class,
            parentColumns = ["id"],
            childColumns = ["accountId"],
            onDelete = ForeignKey.CASCADE,
        ),
    ],
)
data class TransactionAccountInvolvementEntity(
    val transactionTxid: ByteArray,
    val accountId: Long,
    /**
     * This account's own net contribution to the transaction, exactly as
     * upstream reported it for this account — never recomputed from stored
     * coins (that races corrective callbacks; see the netAmountSuspects
     * decision in dashpay/platform#4439). Zero for membership-only rows,
     * such as a provider special transaction that creates no TXO.
     */
    val netAmount: Long = 0,
)
