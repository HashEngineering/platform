package org.dash.sdk.models

import org.dash.sdk.ffi.DashSDKNetwork

enum class Network(val ffiValue: Int) {
    MAINNET(DashSDKNetwork.MAINNET),
    TESTNET(DashSDKNetwork.TESTNET),
    REGTEST(DashSDKNetwork.REGTEST),
    DEVNET(DashSDKNetwork.DEVNET),
    LOCAL(DashSDKNetwork.LOCAL);
}
