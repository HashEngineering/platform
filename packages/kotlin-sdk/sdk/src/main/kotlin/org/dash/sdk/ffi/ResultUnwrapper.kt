package org.dash.sdk.ffi

import com.sun.jna.Pointer
import org.dash.sdk.models.DashSDKException

/**
 * Unwraps a DashSDKResult* pointer returned by FFI calls.
 *
 * Checks for errors, extracts the data pointer, and frees the result.
 * Throws [DashSDKException] if the result contains an error.
 */
internal object ResultUnwrapper {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Unwraps a result pointer, returning the inner data [Pointer] on success.
     * Always frees the result pointer.
     *
     * @param result pointer to a DashSDKResult (may be null)
     * @throws DashSDKException on error or null result
     */
    fun unwrap(result: Pointer?): Pointer {
        if (result == null) {
            throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "FFI returned null result")
        }
        try {
            val errorPtr = ffi.dash_sdk_result_get_error(result)
            if (errorPtr != null) {
                val code = ffi.dash_sdk_error_get_code(errorPtr)
                val msg = ffi.dash_sdk_error_get_message(errorPtr) ?: "Unknown error"
                ffi.dash_sdk_error_free(errorPtr)
                throw DashSDKException(code, msg)
            }
            return ffi.dash_sdk_result_get_data(result)
                ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "Result data is null")
        } finally {
            ffi.dash_sdk_result_free(result)
        }
    }

    /**
     * Unwraps a result pointer and reads it as a UTF-8 C string.
     * Frees both the result and the string pointer.
     */
    fun unwrapString(result: Pointer?): String {
        val dataPtr = unwrap(result)
        val str = dataPtr.getString(0, "UTF-8")
        ffi.dash_sdk_free_string(dataPtr)
        return str
    }

    /**
     * Unwraps a result pointer and returns the inner opaque handle pointer.
     * The caller is responsible for freeing the handle with the appropriate
     * dash_sdk_*_handle_free function.
     */
    fun unwrapHandle(result: Pointer?): Pointer = unwrap(result)
}
