package org.dash.sdk.models

/**
 * Optional tuning for state-transition broadcasts (identity register / top-up / transfer /
 * withdraw). Every field defaults to "use the SDK default" (`0` / `false`), so the default
 * instance is equivalent to passing no settings at all.
 *
 * @param connectTimeoutMs connection-establishment timeout in ms (0 = default)
 * @param timeoutMs per-request timeout in ms (0 = default)
 * @param retries number of retries on failed requests (0 = default)
 * @param banFailedAddress ban a DAPI address that fails to respond
 * @param identityNonceStaleTimeS identity-nonce stale time in seconds (0 = default)
 * @param userFeeIncrease extra percentage added to the processing fee (0 = none); C `uint16`
 * @param allowSigningWithAnySecurityLevel debug: allow signing with any security level
 * @param allowSigningWithAnyPurpose debug: allow signing with any key purpose
 * @param waitTimeoutMs wait-for-confirmation timeout in ms (0 = default)
 */
data class PutSettings(
    val connectTimeoutMs: Long = 0,
    val timeoutMs: Long = 0,
    val retries: Int = 0,
    val banFailedAddress: Boolean = false,
    val identityNonceStaleTimeS: Long = 0,
    val userFeeIncrease: Int = 0,
    val allowSigningWithAnySecurityLevel: Boolean = false,
    val allowSigningWithAnyPurpose: Boolean = false,
    val waitTimeoutMs: Long = 0,
)
