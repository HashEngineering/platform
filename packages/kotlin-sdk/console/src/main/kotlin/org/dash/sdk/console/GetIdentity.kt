package org.dash.sdk.console

import com.sun.jna.Native
import com.sun.jna.Pointer
import org.dash.sdk.ffi.DashSDKConfigNative
import org.dash.sdk.ffi.DashSDKNetwork
import org.dash.sdk.ffi.DashSDKResultNative
import org.dash.sdk.ffi.DashSdkFfi

// ---------------------------------------------------------------------------
// GetIdentity — look up a Dash Platform identity by identity ID, public key
// hash, or non-unique public key hash, and print the result.
//
// Mirrors the self-contained FFI style of DpnsSearch.kt (no DashSDK facade):
// build the config, create a trusted testnet SDK, run the query, free pointers.
// ---------------------------------------------------------------------------

/** How the user wants to find the identity. */
private enum class SearchMode(val menuLabel: String) {
    IDENTITY_ID("Identity ID (hex or base58)"),
    PUBLIC_KEY_HASH("Public key hash — unique (hex)"),
    NON_UNIQUE_PUBLIC_KEY_HASH("Public key hash — non-unique (hex)"),
}

fun main(args: Array<String>) {
    printIdentityBanner()

    // Parse arguments: [--mode id|pkh|npkh] [--dapi <url>] [value]
    var modeArg: String? = null
    var dapiUrl: String? = null
    var value: String? = null
    var i = 0
    while (i < args.size) {
        when {
            args[i] == "--dapi" && i + 1 < args.size -> dapiUrl = args[++i]
            args[i] == "--mode" && i + 1 < args.size -> modeArg = args[++i]
            args[i].startsWith("--") -> { println("Unknown option: ${args[i]}"); printIdentityUsage(); return }
            else -> value = args[i]
        }
        i++
    }

    val ffi = DashSdkFfi.INSTANCE

    val config = DashSDKConfigNative().apply {
        network = DashSDKNetwork.TESTNET
        dapi_addresses = dapiUrl
        skip_asset_lock_proof_verification = 1
        request_retry_count = 3
        request_timeout_ms = 30_000L
    }

    println("\nConnecting to Dash testnet${if (dapiUrl != null) " ($dapiUrl)" else " (trusted nodes)"}...")
    val createResult = if (dapiUrl != null) ffi.dash_sdk_create(config) else ffi.dash_sdk_create_trusted(config)
    val sdkHandle = checkIdentityResult(ffi, createResult, "create SDK") ?: return
    println("Connected.\n")

    try {
        lookupLoop(ffi, sdkHandle, parseMode(modeArg), value)
    } finally {
        ffi.dash_sdk_destroy(sdkHandle)
    }
}

private fun lookupLoop(
    ffi: DashSdkFfi,
    sdkHandle: Pointer,
    initialMode: SearchMode?,
    initialValue: String?,
) {
    var mode = initialMode
    var value = initialValue

    while (true) {
        if (mode == null) {
            mode = promptMode() ?: break
        }
        if (value.isNullOrEmpty()) {
            print("Enter ${mode.menuLabel}: ")
            value = readLine()?.trim()
        }
        if (value.isNullOrEmpty()) {
            println("Error: value cannot be empty.\n")
            mode = null; value = null; continue
        }

        when (mode) {
            SearchMode.IDENTITY_ID -> lookupById(ffi, sdkHandle, value)
            SearchMode.PUBLIC_KEY_HASH -> lookupByPublicKeyHash(ffi, sdkHandle, value)
            SearchMode.NON_UNIQUE_PUBLIC_KEY_HASH -> lookupByNonUniquePublicKeyHash(ffi, sdkHandle, value)
        }

        print("\nLook up another identity? (y/N): ")
        if (readLine()?.trim()?.lowercase() != "y") break
        mode = null; value = null
    }
    println("Goodbye.")
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

private fun lookupById(ffi: DashSdkFfi, sdkHandle: Pointer, identityId: String) {
    println("\nFetching identity \"$identityId\"...")
    // NOTE: dash_sdk_identity_fetch returns a JSON *string*, NOT an IdentityHandle
    // (per rs-sdk-ffi.h: "JSON string representation ... as returned by
    // dash_sdk_identity_fetch"). Use dash_sdk_identity_fetch_handle if a handle is
    // needed; passing this string to identity_get_info/identity_destroy crashes
    // (destroy reads the base58 id text as a BTreeMap and segfaults).
    val result = ffi.dash_sdk_identity_fetch(sdkHandle, identityId)
    printJsonResult(ffi, result, "fetch identity")
}

private fun lookupByPublicKeyHash(ffi: DashSdkFfi, sdkHandle: Pointer, publicKeyHash: String) {
    println("\nFetching identity by public key hash \"$publicKeyHash\"...")
    val result = ffi.dash_sdk_identity_fetch_by_public_key_hash(sdkHandle, publicKeyHash)
    printJsonResult(ffi, result, "fetch by public key hash")
}

private fun lookupByNonUniquePublicKeyHash(ffi: DashSdkFfi, sdkHandle: Pointer, publicKeyHash: String) {
    println("\nFetching identities by non-unique public key hash \"$publicKeyHash\"...")
    val result = ffi.dash_sdk_identity_fetch_by_non_unique_public_key_hash(sdkHandle, publicKeyHash, null)
    printJsonResult(ffi, result, "fetch by non-unique public key hash")
}

private fun printJsonResult(ffi: DashSdkFfi, result: DashSDKResultNative, op: String) {
    val jsonPtr = checkIdentityResult(ffi, result, op) ?: return
    val json = jsonPtr.getString(0, "UTF-8")
    ffi.dash_sdk_string_free(jsonPtr)
    if (json.isBlank() || json == "null") {
        println("  No identity found.")
    } else {
        println("\n  Result:")
        println(prettyJson(json).prependIndent("    "))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

private fun parseMode(arg: String?): SearchMode? = when (arg?.lowercase()) {
    null -> null
    "id", "identity", "identity-id" -> SearchMode.IDENTITY_ID
    "pkh", "public-key-hash" -> SearchMode.PUBLIC_KEY_HASH
    "npkh", "non-unique", "non-unique-public-key-hash" -> SearchMode.NON_UNIQUE_PUBLIC_KEY_HASH
    else -> { println("Unknown mode \"$arg\"; please choose interactively."); null }
}

private fun promptMode(): SearchMode? {
    println("Search for an identity by:")
    SearchMode.entries.forEachIndexed { idx, m -> println("  ${idx + 1}. ${m.menuLabel}") }
    print("Choose 1-${SearchMode.entries.size} (or press Enter to quit): ")
    val choice = readLine()?.trim()
    if (choice.isNullOrEmpty()) return null
    val idx = choice.toIntOrNull()?.minus(1)
    return SearchMode.entries.getOrNull(idx ?: -1) ?: run {
        println("Invalid choice.\n"); promptMode()
    }
}

private fun checkIdentityResult(ffi: DashSdkFfi, result: DashSDKResultNative, op: String): Pointer? {
    val errorPtr = result.error
    if (errorPtr != null) {
        // DashSDKError { enum code (C int @0); char *message (@POINTER_SIZE) }.
        val code = errorPtr.getInt(0)
        val msg = errorPtr.getPointer(Native.POINTER_SIZE.toLong())?.getString(0) ?: "Unknown error"
        ffi.dash_sdk_error_free(errorPtr)
        println("Error ($op) [code $code]: $msg")
        return null
    }
    if (result.data == null) {
        println("Error ($op): returned null with no error details.")
        return null
    }
    return result.data
}

/** Minimal JSON pretty-printer (indents braces/brackets) for readable output. */
private fun prettyJson(json: String): String {
    val sb = StringBuilder()
    var indent = 0
    var inString = false
    var escaped = false
    fun newline() { sb.append('\n'); repeat(indent) { sb.append("  ") } }
    for (c in json.trim()) {
        if (escaped) { sb.append(c); escaped = false; continue }
        when {
            c == '\\' && inString -> { sb.append(c); escaped = true }
            c == '"' -> { sb.append(c); inString = !inString }
            inString -> sb.append(c)
            c == '{' || c == '[' -> { sb.append(c); indent++; newline() }
            c == '}' || c == ']' -> { indent--; newline(); sb.append(c) }
            c == ',' -> { sb.append(c); newline() }
            c == ':' -> sb.append(": ")
            c.isWhitespace() -> {}
            else -> sb.append(c)
        }
    }
    return sb.toString()
}

private fun printIdentityBanner() {
    println("╔═══════════════════════════════════════╗")
    println("║      Dash Platform Get Identity       ║")
    println("╚═══════════════════════════════════════╝")
}

private fun printIdentityUsage() {
    println("\nUsage: get-identity [--mode id|pkh|npkh] [--dapi <url>] [value]")
    println("  --mode <mode> Search mode:")
    println("                  id    Identity ID (hex or base58)")
    println("                  pkh   Public key hash (unique)")
    println("                  npkh  Public key hash (non-unique)")
    println("  --dapi <url>  DAPI node URL (default: trusted testnet nodes)")
    println("  value         The identity ID or public key hash to look up")
    println("\nExamples:")
    println("  get-identity --mode id 5oVAjwfDGwLR2g2bYxBBxBHvqYzKgK7eZqJ7XkgJ5R8A")
    println("  get-identity --mode pkh b7e9f...  --dapi https://testnet.dash.org:3000")
    println("\nWith no arguments, runs in interactive mode.")
}