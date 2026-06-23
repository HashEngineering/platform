package org.dash.sdk.ffi

import org.dash.sdk.models.PutSettings

/**
 * Marshal [PutSettings] into a native [DashSDKPutSettingsNative] (already [write]-flushed).
 *
 * Shared by every state-transition write wrapper (identity / document / token / DPNS).
 * The returned struct must be kept reachable until the FFI call returns — callers pass
 * `settings?.toNative()?.pointer` and `Reference.reachabilityFence` the struct.
 * Pass `null` (omit the settings) for the FFI's all-defaults behavior.
 */
internal fun PutSettings.toNative(): DashSDKPutSettingsNative =
    DashSDKPutSettingsNative().apply {
        connect_timeout_ms = connectTimeoutMs
        timeout_ms = timeoutMs
        retries = this@toNative.retries
        ban_failed_address = if (banFailedAddress) 1 else 0
        identity_nonce_stale_time_s = identityNonceStaleTimeS
        user_fee_increase = userFeeIncrease.toShort()
        allow_signing_with_any_security_level = if (allowSigningWithAnySecurityLevel) 1 else 0
        allow_signing_with_any_purpose = if (allowSigningWithAnyPurpose) 1 else 0
        wait_timeout_ms = waitTimeoutMs
        write()
    }
