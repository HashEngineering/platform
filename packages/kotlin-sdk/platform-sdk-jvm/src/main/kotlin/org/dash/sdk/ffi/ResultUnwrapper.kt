package org.dash.sdk.ffi

import com.sun.jna.Pointer
import org.dash.sdk.models.DashSDKException

/**
 * Unwraps a [DashSDKResultNative] returned by-value from FFI calls.
 *
 * All SDK functions return DashSDKResult as a C struct by value.
 * On success error == null and data holds the payload pointer.
 * On failure error != null and data is null.
 */
internal object ResultUnwrapper {

    private val ffi get() = DashSdkFfi.INSTANCE

    /**
     * Unwraps a by-value result, returning the inner data [Pointer] on success.
     * Frees the error struct if present.
     *
     * @throws DashSDKException on error or null data
     */
    fun unwrap(result: DashSDKResultNative): Pointer {
        val errorPtr = result.error
        if (errorPtr != null) {
            // DashSDKError { enum code (C int @0); char *message (@POINTER_SIZE) }.
            // Read the struct fields directly — the header exposes no error accessor
            // functions (dash_sdk_error_get_code/_get_message do not exist in the
            // library); only dash_sdk_error_free is exported. This mirrors DashSDK.create.
            val code = errorPtr.getInt(0)
            val msgPtr = errorPtr.getPointer(com.sun.jna.Native.POINTER_SIZE.toLong())
            val msg = msgPtr?.getString(0) ?: "Unknown error"
            ffi.dash_sdk_error_free(errorPtr)
            throw DashSDKException(code, msg)
        }
        return result.data
            ?: throw DashSDKException(DashSDKErrorCode.INTERNAL_ERROR, "Result data is null")
    }

    /**
     * Unwraps a result that carries no data payload on success: throws on error, otherwise
     * returns Unit. Used by write paths whose Rust side returns `DashSDKResult::success(null)`
     * (e.g. the token state-transition ops — `mint`/`burn`/`transfer`/`freeze`/`unfreeze`),
     * where a null `data` is a successful outcome rather than the missing-data error that
     * [unwrap] reports.
     *
     * @throws DashSDKException only when the FFI reports an error
     */
    fun unwrapVoid(result: DashSDKResultNative) {
        val errorPtr = result.error
        if (errorPtr != null) {
            val code = errorPtr.getInt(0)
            val msgPtr = errorPtr.getPointer(com.sun.jna.Native.POINTER_SIZE.toLong())
            val msg = msgPtr?.getString(0) ?: "Unknown error"
            ffi.dash_sdk_error_free(errorPtr)
            throw DashSDKException(code, msg)
        }
        // Success with a null/opaque data payload — nothing to read or free.
    }

    /**
     * Unwraps a result and reads the data pointer as a UTF-8 C string.
     * Frees the string pointer after reading.
     */
    fun unwrapString(result: DashSDKResultNative): String {
        val dataPtr = unwrap(result)
        val str = dataPtr.getString(0, "UTF-8")
        ffi.dash_sdk_string_free(dataPtr)
        return str
    }

    /**
     * Unwraps a result and returns the inner opaque handle pointer.
     * The caller is responsible for freeing the handle.
     */
    fun unwrapHandle(result: DashSDKResultNative): Pointer = unwrap(result)
}
