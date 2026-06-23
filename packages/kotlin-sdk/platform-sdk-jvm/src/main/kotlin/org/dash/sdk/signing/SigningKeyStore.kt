package org.dash.sdk.signing

import java.util.concurrent.ConcurrentHashMap

/**
 * Maps a public key to the 32-byte ECDSA private scalar that signs for it.
 *
 * Used by any signing/write operation — identity registration, token transfers, document
 * writes, contract updates, etc. The signer ([KeystoreSigner]) looks keys up by **raw
 * public-key bytes** — the value the Rust signer passes through the sign/can-sign
 * callbacks — not by identity ID, so a store can be populated before an identity is
 * registered (the on-chain ID is derived later from the asset-lock proof).
 *
 * The concrete secure backing (encrypted file / Jetpack Security on Android) is still
 * deferred; [InMemorySigningKeyStore] is provided for tests, tooling, and the console.
 * There is no secp256k1 hardware path on the JVM/Android, so the private scalar is
 * app-managed material in every implementation.
 */
interface SigningKeyStore {

    /**
     * Return a copy of the 32-byte ECDSA private scalar for [publicKey], or null if this
     * store holds no key for it.
     */
    fun privateKeyFor(publicKey: ByteArray): ByteArray?

    /** True iff a private key is available for [publicKey]. */
    fun hasKeyFor(publicKey: ByteArray): Boolean = privateKeyFor(publicKey) != null
}

/**
 * In-memory [SigningKeyStore] keyed by the hex of the public-key bytes. Thread-safe.
 * Holds raw scalars in the heap — suitable for tests/tooling, not durable storage.
 */
class InMemorySigningKeyStore : SigningKeyStore {

    private val keys = ConcurrentHashMap<String, ByteArray>()

    /**
     * Register a (public key → 32-byte private scalar) pair.
     *
     * @throws IllegalArgumentException if [privateKey] is not exactly 32 bytes or
     *   [publicKey] is empty
     */
    fun put(publicKey: ByteArray, privateKey: ByteArray) {
        require(publicKey.isNotEmpty()) { "publicKey must not be empty" }
        require(privateKey.size == 32) { "privateKey must be 32 bytes, was ${privateKey.size}" }
        keys[publicKey.toHexKey()] = privateKey.copyOf()
    }

    override fun privateKeyFor(publicKey: ByteArray): ByteArray? =
        keys[publicKey.toHexKey()]?.copyOf()

    override fun hasKeyFor(publicKey: ByteArray): Boolean =
        keys.containsKey(publicKey.toHexKey())

    private fun ByteArray.toHexKey(): String {
        val sb = StringBuilder(size * 2)
        for (b in this) sb.append("%02x".format(b.toInt() and 0xFF))
        return sb.toString()
    }
}
