package org.dash.sdk.models

/**
 * Components of one identity public key, used to build an identity via
 * [org.dash.sdk.services.IdentityService.createFromComponents].
 *
 * Discriminants follow the DPP `repr(u8)` enums:
 * - [keyType]: 0 = ECDSA_SECP256K1, 1 = BLS12_381, 2 = ECDSA_HASH160,
 *   3 = BIP13_SCRIPT_HASH, 4 = EDDSA_25519_HASH160
 * - [purpose]: 0 = AUTHENTICATION, 1 = ENCRYPTION, 2 = DECRYPTION, 3 = TRANSFER, …
 * - [securityLevel]: 0 = MASTER, 1 = CRITICAL, 2 = HIGH, 3 = MEDIUM
 *
 * @param keyId key ID (0-255)
 * @param data raw public-key bytes (33 for compressed secp256k1)
 * @param disabledAt disabled timestamp in ms (0 = enabled)
 */
class IdentityPublicKeyParams(
    val keyId: Int,
    val keyType: Int,
    val purpose: Int,
    val securityLevel: Int,
    val data: ByteArray,
    val readOnly: Boolean = false,
    val disabledAt: Long = 0L,
) {
    init {
        require(keyId in 0..255) { "keyId must be 0..255, was $keyId" }
        require(data.isNotEmpty()) { "public key data must not be empty" }
    }
}
