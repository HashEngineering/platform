package org.dash.sdk.models

import org.dash.sdk.ffi.DashSDKErrorCode

/**
 * Exception thrown when an FFI call fails.
 */
class DashSDKException(
    val errorCode: Int,
    message: String
) : Exception(message) {

    val isNotFound: Boolean get() = errorCode == DashSDKErrorCode.NOT_FOUND
    val isNetworkError: Boolean get() = errorCode == DashSDKErrorCode.NETWORK_ERROR
    val isTimeout: Boolean get() = errorCode == DashSDKErrorCode.TIMEOUT

    override fun toString(): String = "DashSDKException(code=$errorCode, message=$message)"
}
