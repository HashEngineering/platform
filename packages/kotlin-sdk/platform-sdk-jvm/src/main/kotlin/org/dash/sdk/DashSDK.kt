package org.dash.sdk

import com.sun.jna.Pointer
import org.dash.sdk.ffi.DashSDKConfigNative
import org.dash.sdk.ffi.DashSDKResultNative
import org.dash.sdk.ffi.DashSdkFfi
import org.dash.sdk.ffi.NativeLoader
import org.dash.sdk.models.DashSDKException
import org.dash.sdk.models.Network
import org.dash.sdk.services.DataContractService
import org.dash.sdk.services.DocumentService
import org.dash.sdk.services.DpnsService
import org.dash.sdk.services.IdentityService
import org.dash.sdk.services.SystemService
import org.dash.sdk.services.TokenService

/**
 * Main entry point for the Dash Platform Android SDK.
 *
 * Wraps the Rust [rs-sdk-ffi] library loaded via JNA, providing
 * coroutine-friendly access to identity, document, contract, and DPNS operations.
 *
 * Usage:
 * ```kotlin
 * val sdk = DashSDK.create(
 *     network = Network.TESTNET,
 *     dapiAddresses = "http://127.0.0.1:3000"
 * )
 * val identity = sdk.identity.fetchIdentity("aabbcc...")
 * sdk.close()
 * ```
 *
 * Implements [AutoCloseable] — use with Kotlin's `use {}` or close manually.
 */
class DashSDK private constructor(
    private val handle: Pointer,
    val network: Network
) : AutoCloseable {

    private val ffi get() = DashSdkFfi.INSTANCE

    /** Identity fetch and query operations. */
    val identity: IdentityService = IdentityService(handle)

    /** Data contract fetch operations. */
    val dataContract: DataContractService = DataContractService(handle)

    /** Document fetch and search operations. */
    val document: DocumentService = DocumentService(handle)

    /** Dash Platform Naming Service (DPNS) operations. */
    val dpns: DpnsService = DpnsService(handle)

    /** System / status / protocol-version read queries. */
    val system: SystemService = SystemService(handle)

    /** Token read queries (balances, info, prices, supply, distributions). */
    val token: TokenService = TokenService(handle)

    /** Release native resources. Safe to call multiple times. */
    override fun close() {
        ffi.dash_sdk_destroy(handle)
    }

    companion object {

        /**
         * Create and initialize a new [DashSDK] instance.
         *
         * @param network which Dash network to connect to
         * @param dapiAddresses comma-separated DAPI node URLs,
         *        e.g. `"http://127.0.0.1:3000,http://127.0.0.1:3001"`.
         *        Pass null or empty string to use the mock SDK.
         * @param requestTimeoutMs timeout for individual requests (ms)
         * @param connectTimeoutMs timeout for establishing connections (ms)
         * @param skipAssetLockProofVerification skip asset lock proof checks (testing only)
         * @throws DashSDKException if the native SDK could not be initialized
         */
        fun create(
            network: Network = Network.TESTNET,
            dapiAddresses: String? = null,
            requestTimeoutMs: Long = 30_000L,
            requestRetryCount: Int = 3,
            skipAssetLockProofVerification: Boolean = false,
        ): DashSDK {
            NativeLoader.load()

            val config = DashSDKConfigNative().apply {
                this.network = network.ffiValue
                this.dapi_addresses = dapiAddresses
                this.request_timeout_ms = requestTimeoutMs
                this.request_retry_count = requestRetryCount
                this.skip_asset_lock_proof_verification =
                    if (skipAssetLockProofVerification) 1 else 0
            }

            val result: DashSDKResultNative = DashSdkFfi.INSTANCE.dash_sdk_create(config)
            if (result.error != null) {
                val code = result.error!!.getInt(0)
                val msgPtr = result.error!!.getPointer(com.sun.jna.Native.POINTER_SIZE.toLong())
                val msg = msgPtr?.getString(0) ?: "Unknown error"
                throw DashSDKException(errorCode = code, message = msg)
            }
            val handle = result.data
                ?: throw DashSDKException(errorCode = 99, message = "Failed to create native SDK handle")

            return DashSDK(handle, network)
        }
    }
}
