package org.dashfoundation.dashsdk.wallet

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the reconcile-diagnostics decoders against the JNI serializers in
 * `rs-unified-sdk-jni/src/wallet_manager.rs` (`coreWalletState` long[3] and
 * `accountSpentOutpoints` 36-bytes-per-row blob).
 */
class CoreWalletStateTest {

    @Test
    fun coreWalletStateDecodesThreeLongs() {
        val state = CoreWalletState.fromLongArray(longArrayOf(1_546_911L, 1_546_900L, 42L))
        assertEquals(1_546_911L, state.syncedHeight)
        assertEquals(1_546_900L, state.lastProcessedHeight)
        assertEquals(42L, state.monitorRevision)
    }

    @Test
    fun coreWalletStateRejectsWrongArity() {
        assertThrows(IllegalArgumentException::class.java) {
            CoreWalletState.fromLongArray(longArrayOf(1L, 2L))
        }
    }

    @Test
    fun spentOutpointsDecodeEmptyBlob() {
        assertTrue(OutPoint.decodeList(ByteArray(0)).isEmpty())
    }

    @Test
    fun spentOutpointsDecodeRoundTrip() {
        val txidA = ByteArray(32) { it.toByte() }
        val txidB = ByteArray(32) { (255 - it).toByte() }
        val buf = ByteBuffer.allocate(2 * OutPoint.BLOB_STRIDE).order(ByteOrder.BIG_ENDIAN)
        buf.put(txidA); buf.putInt(0)
        buf.put(txidB); buf.putInt(0xFFFFFFFF.toInt()) // max vout, must stay unsigned
        val list = OutPoint.decodeList(buf.array())
        assertEquals(2, list.size)
        assertEquals(OutPoint(txidA, 0L), list[0])
        assertEquals(OutPoint(txidB, 0xFFFFFFFFL), list[1])
        // u32 vout must not sign-extend to a negative Long.
        assertTrue(list[1].vout > 0)
    }

    @Test
    fun spentOutpointsRejectMisalignedBlob() {
        assertThrows(IllegalArgumentException::class.java) {
            OutPoint.decodeList(ByteArray(OutPoint.BLOB_STRIDE + 1))
        }
    }

    @Test
    fun outpointRejectsWrongTxidSize() {
        assertThrows(IllegalArgumentException::class.java) {
            OutPoint(ByteArray(31), 0L)
        }
    }
}
