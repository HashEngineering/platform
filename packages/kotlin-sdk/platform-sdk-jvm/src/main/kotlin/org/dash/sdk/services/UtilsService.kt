package org.dash.sdk.services

import com.sun.jna.Memory
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.ResultUnwrapper
import org.dash.sdk.models.Network

/**
 * Process-local utility helpers (base58/hex conversion, platform-address encoding,
 * GroveDB proof formatting). None of these take an SDK handle or touch the network, so
 * every method is synchronous.
 *
 * Each method is a 1:1 wrapper over a single `rs-sdk-ffi` call (marshal in → call →
 * marshal out) — no business logic, per the SDK's persist/load/bridge contract.
 */
class UtilsService internal constructor() {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Convert a base58 string to its hex encoding.
     *
     * @param base58 base58-encoded string
     * @return hex-encoded string
     */
    fun base58ToHex(base58: String): String =
        ResultUnwrapper.unwrapString(ffi.dash_sdk_utils_base58_to_hex(base58))

    /**
     * Convert a hex string to its base58 encoding.
     *
     * @param hex hex-encoded string
     * @return base58-encoded string
     */
    fun hexToBase58(hex: String): String =
        ResultUnwrapper.unwrapString(ffi.dash_sdk_utils_hex_to_base58(hex))

    /**
     * Validate whether [string] is valid base58.
     *
     * Backed by an FFI function that returns a raw `uint8_t` (not a `DashSDKResult`),
     * so this never throws — it returns `false` for invalid input.
     */
    fun isValidBase58(string: String): Boolean =
        ffi.dash_sdk_utils_is_valid_base58(string) != 0.toByte()

    /**
     * Encode a P2PKH scriptPubKey as a bech32m platform address (DIP-18).
     *
     * @param scriptPubKey raw scriptPubKey bytes (a 25-byte P2PKH script)
     * @param network the network whose address version to encode for
     * @return bech32m platform-address string
     * @throws IllegalArgumentException if [scriptPubKey] is empty
     */
    fun encodePlatformAddress(scriptPubKey: ByteArray, network: Network): String {
        require(scriptPubKey.isNotEmpty()) { "scriptPubKey must not be empty" }
        val buffer = Memory(scriptPubKey.size.toLong())
        buffer.write(0, scriptPubKey, 0, scriptPubKey.size)
        return ResultUnwrapper.unwrapString(
            ffi.dash_sdk_encode_platform_address(buffer, scriptPubKey.size, network.ffiNetworkValue)
        )
    }

    /**
     * Format a raw GroveDB proof as a human-readable, tree-structured visualization.
     *
     * @param proof raw GroveDB proof bytes
     * @return tree-structured string visualization
     * @throws IllegalArgumentException if [proof] is empty
     */
    fun formatGrovedbProof(proof: ByteArray): String {
        require(proof.isNotEmpty()) { "proof must not be empty" }
        val buffer = Memory(proof.size.toLong())
        buffer.write(0, proof, 0, proof.size)
        return ResultUnwrapper.unwrapString(
            ffi.dash_sdk_format_grovedb_proof(buffer, proof.size)
        )
    }

    /**
     * Derive the public-key bytes (hex) for a private key. Process-local — no network.
     *
     * @param privateKeyHex hex-encoded 32-byte ECDSA private scalar
     * @param keyType DPP KeyType discriminant (0 = ECDSA_SECP256K1)
     * @param network network whose key encoding to use
     * @return hex-encoded public-key bytes
     */
    fun publicKeyFromPrivateKey(
        privateKeyHex: String,
        keyType: Int = 0,
        network: Network = Network.TESTNET,
    ): String = ResultUnwrapper.unwrapString(
        ffi.dash_sdk_public_key_data_from_private_key_data(privateKeyHex, keyType.toByte(), network.ffiNetworkValue)
    )

    /**
     * Validate that a private key corresponds to a public key. Process-local — no network.
     *
     * @param privateKeyHex hex-encoded private scalar
     * @param publicKeyHex hex-encoded public key
     * @param keyType DPP KeyType discriminant (0 = ECDSA_SECP256K1)
     * @return the FFI's status string
     */
    fun validatePrivateKeyForPublicKey(
        privateKeyHex: String,
        publicKeyHex: String,
        keyType: Int = 0,
        network: Network = Network.TESTNET,
    ): String = ResultUnwrapper.unwrapString(
        ffi.dash_sdk_validate_private_key_for_public_key(
            privateKeyHex, publicKeyHex, keyType.toByte(), network.ffiNetworkValue
        )
    )
}
