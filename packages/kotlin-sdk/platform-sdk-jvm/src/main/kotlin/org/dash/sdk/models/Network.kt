package org.dash.sdk.models

import org.dash.sdk.ffi.DashSDKNetwork
import org.dash.sdk.ffi.FFINetwork

enum class Network(val ffiValue: Int) {
    MAINNET(DashSDKNetwork.MAINNET),
    TESTNET(DashSDKNetwork.TESTNET),
    REGTEST(DashSDKNetwork.REGTEST),
    DEVNET(DashSDKNetwork.DEVNET),
    LOCAL(DashSDKNetwork.LOCAL);

    /**
     * Map to the `FFINetwork` enum value used by FFI functions that type their network
     * parameter as `FFINetwork` (e.g. `dash_sdk_encode_platform_address`). This enum differs
     * from [DashSDKNetwork] (Devnet/Regtest are swapped; there is no Local), so it is NOT the
     * same as [ffiValue]. [LOCAL] has no `FFINetwork` equivalent and is mapped to Regtest
     * (the closest local-chain variant).
     */
    val ffiNetworkValue: Int
        get() = when (this) {
            MAINNET -> FFINetwork.MAINNET
            TESTNET -> FFINetwork.TESTNET
            DEVNET -> FFINetwork.DEVNET
            REGTEST -> FFINetwork.REGTEST
            LOCAL -> FFINetwork.REGTEST
        }
}
