package org.dashfoundation.dashsdk.wallet

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Per-wallet Core SPV state — the read half of the rescan diagnostics the
 * host-side backfill gate otherwise has to infer. Decoded from
 * `WalletManagerNative.coreWalletState`.
 *
 * @property syncedHeight the filter-scan checkpoint — the field
 *   `spvRescanFilters` rewinds and the filter pipeline advances. Reading it
 *   before and after arming a rescan is the direct way to confirm a rewind
 *   actually took (rather than being stomped back to tip by an in-flight
 *   pipeline commit, the MO-1012 in-session suppression).
 * @property lastProcessedHeight the monotonic processed watermark.
 * @property monitorRevision the wallet's monitored-script-set revision.
 */
data class CoreWalletState(
    val syncedHeight: Long,
    val lastProcessedHeight: Long,
    val monitorRevision: Long,
) {
    companion object {
        fun fromLongArray(values: LongArray): CoreWalletState {
            require(values.size == 3) {
                "coreWalletState must return 3 longs, got ${values.size}"
            }
            return CoreWalletState(
                syncedHeight = values[0],
                lastProcessedHeight = values[1],
                monitorRevision = values[2],
            )
        }
    }
}

/** A transaction outpoint: 32-byte txid (raw internal order) + vout. */
data class OutPoint(val txid: ByteArray, val vout: Long) {
    init {
        require(txid.size == 32) { "txid must be 32 bytes, got ${txid.size}" }
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is OutPoint) return false
        return vout == other.vout && txid.contentEquals(other.txid)
    }

    override fun hashCode(): Int = 31 * txid.contentHashCode() + vout.hashCode()

    companion object {
        /** Bytes per outpoint in the `accountSpentOutpoints` blob. */
        const val BLOB_STRIDE: Int = 36

        /**
         * Decode the flat big-endian blob from
         * `WalletManagerNative.accountSpentOutpoints` — 36 bytes per row,
         * 32-byte txid then u32 vout.
         */
        fun decodeList(blob: ByteArray): List<OutPoint> {
            require(blob.size % BLOB_STRIDE == 0) {
                "spent-outpoint blob length ${blob.size} is not a multiple of $BLOB_STRIDE"
            }
            val count = blob.size / BLOB_STRIDE
            val buf = ByteBuffer.wrap(blob).order(ByteOrder.BIG_ENDIAN)
            return buildList(count) {
                repeat(count) {
                    val txid = ByteArray(32).also { buf.get(it) }
                    val vout = buf.int.toLong() and 0xFFFFFFFFL
                    add(OutPoint(txid, vout))
                }
            }
        }
    }
}
