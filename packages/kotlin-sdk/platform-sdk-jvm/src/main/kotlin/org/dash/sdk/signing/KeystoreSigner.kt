package org.dash.sdk.signing

import com.sun.jna.Memory
import com.sun.jna.NativeLong
import com.sun.jna.Pointer
import org.dash.sdk.ffi.CanSignCallback
import org.dash.sdk.ffi.DashSDKErrorCode
import org.dash.sdk.ffi.DashSDKSignatureNative
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.DestroyCallback
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.ffi.SignAsyncCallback
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network

/**
 * An external signer that can be handed to identity write operations (the
 * `*_with_signer` / put-to-platform FFI entry points) via [handle].
 *
 * The underlying `*mut SignerHandle` is owned by this object and freed by [close];
 * the owner must keep the [Signer] alive for the duration of any FFI call that captured
 * [handle].
 */
interface Signer : AutoCloseable {
    /** Raw `*mut SignerHandle` for the FFI. Owned by this object; freed in [close]. */
    val handle: Pointer
}

/**
 * Production-shaped [Signer] backed by a [SigningKeyStore].
 *
 * # How a signing request flows
 * 1. Rust calls the C `sign_async` trampoline with the **raw** public-key bytes, the
 *    DPP `KeyType` byte, and the data to sign.
 * 2. We look the private key up in [keyStore] by those public-key bytes.
 * 3. We wrap the scalar in a throwaway FFI signer
 *    ([DashSdkFfi.dash_sdk_signer_create_from_private_key]), produce a signature
 *    ([DashSdkFfi.dash_sdk_signer_sign]), destroy that signer, and scrub the key buffer.
 * 4. The signature is shipped back via [DashSdkFfi.dash_sdk_sign_async_completion].
 *
 * v1 round-trips through the FFI sign primitive rather than pulling in a native-Kotlin
 * secp256k1 dependency, matching the Swift `KeychainSigner` v1.
 *
 * # JNA lifetime contract
 * The three [com.sun.jna.Callback] instances are held as `private val` fields so JNA does
 * not collect the native trampolines while Rust still holds the vtable. The destroy
 * callback is a no-op (state is GC-managed); [close] frees the Rust handle. Callbacks may
 * fire from any native worker thread.
 */
class KeystoreSigner(
    private val keyStore: SigningKeyStore,
    private val network: Network = Network.TESTNET,
) : Signer {

    private val ffi get() = DashSdkFfi.INSTANCE

    // Held as fields so JNA does not GC the native trampolines (see class doc).
    private val signCallback = SignAsyncCallback { _, pubkeyBytes, pubkeyLen, _, data, dataLen, completionCtx, _ ->
        try {
            val pub = pubkeyBytes?.getByteArray(0, pubkeyLen.toInt()) ?: ByteArray(0)
            val msg = data?.getByteArray(0, dataLen.toInt()) ?: ByteArray(0)
            val priv = keyStore.privateKeyFor(pub)
                ?: throw DashSDKException(DashSDKErrorCode.INVALID_PARAMETER, "No private key for the requested public key")
            val signature = signWithPrivateKey(priv, msg)
            val sigMem = Memory(signature.size.toLong())
            sigMem.write(0, signature, 0, signature.size)
            ffi.dash_sdk_sign_async_completion(completionCtx, sigMem, NativeLong(signature.size.toLong()), null)
        } catch (e: Throwable) {
            ffi.dash_sdk_sign_async_completion(completionCtx, null, NativeLong(0), e.message ?: "sign failed")
        }
    }

    private val canSignCallback = CanSignCallback { _, pubkeyBytes, pubkeyLen, _ ->
        val pub = pubkeyBytes?.getByteArray(0, pubkeyLen.toInt()) ?: ByteArray(0)
        keyStore.hasKeyFor(pub)
    }

    // No-op: init uses the no-ctx create, so there is no extra retain to release.
    private val destroyCallback = DestroyCallback { }

    override val handle: Pointer =
        ffi.dash_sdk_signer_create(signCallback, canSignCallback, destroyCallback)
            ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "dash_sdk_signer_create returned NULL")

    /**
     * True iff this signer can sign for [publicKey]. Routes through the native
     * `can_sign` vtable entry, exercising the full callback round-trip.
     *
     * @param keyType DPP KeyType discriminant (0 = ECDSA_SECP256K1)
     */
    fun canSign(publicKey: ByteArray, keyType: Byte = 0): Boolean {
        require(publicKey.isNotEmpty()) { "publicKey must not be empty" }
        val mem = Memory(publicKey.size.toLong())
        mem.write(0, publicKey, 0, publicKey.size)
        return ffi.dash_sdk_signer_can_sign(handle, mem, NativeLong(publicKey.size.toLong()), keyType)
    }

    /**
     * Look up the private key for [publicKey] and sign [data]. Exposed for callers (and
     * tests) that want to drive the v1 sign primitive directly without the async vtable.
     *
     * @throws DashSDKException if no key is held for [publicKey] or signing fails
     */
    fun sign(publicKey: ByteArray, data: ByteArray): ByteArray {
        val priv = keyStore.privateKeyFor(publicKey)
            ?: throw DashSDKException(DashSDKErrorCode.INVALID_PARAMETER, "No private key for the requested public key")
        return signWithPrivateKey(priv, data)
    }

    /**
     * v1 sign primitive: wrap [privateKey] in a throwaway FFI signer, sign [data], free
     * the signer and signature, and scrub the key buffer. The key material window is the
     * single FFI call.
     */
    private fun signWithPrivateKey(privateKey: ByteArray, data: ByteArray): ByteArray {
        val keyMem = Memory(privateKey.size.toLong())
        try {
            keyMem.write(0, privateKey, 0, privateKey.size)
            val signerHandle = ResultUnwrapper.unwrapHandle(
                ffi.dash_sdk_signer_create_from_private_key(
                    keyMem, NativeLong(privateKey.size.toLong()), network.ffiNetworkValue
                )
            )
            try {
                // Memory(0) is illegal; use a 1-byte scratch when signing empty data and pass len 0.
                val dataMem = Memory(maxOf(1, data.size).toLong())
                if (data.isNotEmpty()) dataMem.write(0, data, 0, data.size)
                val sigPtr = ResultUnwrapper.unwrapHandle(
                    ffi.dash_sdk_signer_sign(signerHandle, dataMem, NativeLong(data.size.toLong()))
                )
                try {
                    val sig = DashSDKSignatureNative(sigPtr)
                    sig.read()
                    val len = sig.signature_len.toInt()
                    return sig.signature?.getByteArray(0, len) ?: ByteArray(0)
                } finally {
                    ffi.dash_sdk_signature_free(sigPtr)
                }
            } finally {
                ffi.dash_sdk_signer_destroy(signerHandle)
            }
        } finally {
            keyMem.clear() // best-effort scrub of the key material
        }
    }

    override fun close() {
        ffi.dash_sdk_signer_destroy(handle)
    }
}
