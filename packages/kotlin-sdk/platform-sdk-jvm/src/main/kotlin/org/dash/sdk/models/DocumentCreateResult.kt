package org.dash.sdk.models

import com.sun.jna.Pointer

/**
 * Result of [org.dash.sdk.services.DocumentService.createDocument]: a process-local
 * DocumentHandle plus the 32-byte entropy the create used for document-ID generation.
 *
 * The [handle] is owned by the caller and must be freed with
 * [org.dash.sdk.services.DocumentService.destroyDocument]. The [entropy] is the exact value
 * that must be supplied to
 * [org.dash.sdk.services.DocumentService.putToPlatform] for this document (the FFI does not
 * re-derive it).
 *
 * @property handle opaque `DocumentHandle *`
 * @property entropy 32-byte entropy used for the document ID
 */
data class DocumentCreateResult(
    val handle: Pointer,
    val entropy: ByteArray
) {
    init {
        require(entropy.size == 32) { "entropy must be 32 bytes, was ${entropy.size}" }
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is DocumentCreateResult) return false
        return handle == other.handle && entropy.contentEquals(other.entropy)
    }

    override fun hashCode(): Int = 31 * handle.hashCode() + entropy.contentHashCode()
}
