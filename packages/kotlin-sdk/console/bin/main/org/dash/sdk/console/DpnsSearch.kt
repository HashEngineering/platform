package org.dash.sdk.console

import com.sun.jna.Pointer
import org.dash.sdk.ffi.DashSDKConfigNative
import org.dash.sdk.ffi.DashSDKNetwork
import org.dash.sdk.ffi.DashSDKResultNative
import org.dash.sdk.ffi.DashSdkFfi

// ---------------------------------------------------------------------------
// Simple JSON helpers (no external library needed)
// ---------------------------------------------------------------------------

/** Extracts the string value of [key] from a JSON object string like {"key":"value",...}. */
private fun jsonString(obj: String, key: String): String? {
    val pattern = Regex(""""$key"\s*:\s*"([^"\\]*(?:\\.[^"\\]*)*)"""")
    return pattern.find(obj)?.groupValues?.get(1)
}

/** Splits a JSON array string into individual object strings. */
private fun splitJsonArray(json: String): List<String> {
    val trimmed = json.trim()
    if (trimmed == "[]" || trimmed.isEmpty()) return emptyList()

    val objects = mutableListOf<String>()
    var depth = 0
    var start = -1
    for (i in trimmed.indices) {
        when (trimmed[i]) {
            '{' -> { if (depth++ == 0) start = i }
            '}' -> { if (--depth == 0 && start >= 0) objects += trimmed.substring(start, i + 1) }
        }
    }
    return objects
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fun main(args: Array<String>) {
    printBanner()

    // Parse arguments: [prefix] [--dapi <url>]
    var prefix: String? = null
    var dapiUrl: String? = null
    var i = 0
    while (i < args.size) {
        when {
            args[i] == "--dapi" && i + 1 < args.size -> { dapiUrl = args[++i] }
            args[i].startsWith("--") -> { println("Unknown option: ${args[i]}"); printUsage(); return }
            else -> prefix = args[i]
        }
        i++
    }

    // Interactive mode if no prefix provided
    if (prefix == null) {
        print("Enter username prefix to search: ")
        prefix = readLine()?.trim()
    }
    if (prefix.isNullOrEmpty()) {
        println("Error: prefix cannot be empty.")
        printUsage()
        return
    }

    val ffi = DashSdkFfi.INSTANCE

    // Build config
    val config = DashSDKConfigNative().apply {
        network = DashSDKNetwork.TESTNET
        dapi_addresses = dapiUrl
        skip_asset_lock_proof_verification = 1
        request_retry_count = 3
        request_timeout_ms = 30_000L
    }

    // Create SDK
    println("\nConnecting to Dash testnet${if (dapiUrl != null) " ($dapiUrl)" else " (trusted nodes)"}...")
    val createResult = if (dapiUrl != null) ffi.dash_sdk_create(config) else ffi.dash_sdk_create_trusted(config)

    val sdkHandle = checkResult(ffi, createResult, "create SDK") ?: return
    println("Connected.\n")

    try {
        searchLoop(ffi, sdkHandle, prefix)
    } finally {
        ffi.dash_sdk_destroy(sdkHandle)
    }
}

private fun searchLoop(ffi: DashSdkFfi, sdkHandle: Pointer, initialPrefix: String) {
    var prefix = initialPrefix
    while (true) {
        println("Searching for usernames starting with \"$prefix\"...")
        val searchResult = ffi.dash_sdk_dpns_search(sdkHandle, prefix, 20)
        val jsonPtr = checkResult(ffi, searchResult, "search") ?: break

        val json = jsonPtr.getString(0, "UTF-8")
        ffi.dash_sdk_string_free(jsonPtr)

        val results = splitJsonArray(json)
        if (results.isEmpty()) {
            println("  No usernames found for prefix \"$prefix\".")
        } else {
            println("  Found ${results.size} result(s):\n")
            results.forEachIndexed { idx, obj ->
                val label    = jsonString(obj, "label")        ?: "?"
                val fullName = jsonString(obj, "fullName")     ?: "?"
                val ownerId  = jsonString(obj, "ownerId")      ?: "?"
                println("  ${idx + 1}. $fullName")
                println("     Label:   $label")
                println("     Owner:   $ownerId")
                println()
            }
        }

        print("Search again (enter new prefix, or press Enter to quit): ")
        val next = readLine()?.trim()
        if (next.isNullOrEmpty()) break
        prefix = next
    }
    println("Goodbye.")
}

private fun checkResult(ffi: DashSdkFfi, result: DashSDKResultNative, op: String): Pointer? {
    val errorPtr = result.error
    if (errorPtr != null) {
        val code = ffi.dash_sdk_error_get_code(errorPtr)
        val msg  = ffi.dash_sdk_error_get_message(errorPtr) ?: "Unknown error"
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

private fun printBanner() {
    println("╔═══════════════════════════════════════╗")
    println("║      Dash Platform DPNS Search        ║")
    println("╚═══════════════════════════════════════╝")
}

private fun printUsage() {
    println("\nUsage: dpns-search [prefix] [--dapi <url>]")
    println("  prefix        Username prefix to search (e.g. \"ali\")")
    println("  --dapi <url>  DAPI node URL (default: trusted testnet nodes)")
    println("\nExamples:")
    println("  dpns-search alice")
    println("  dpns-search ali --dapi https://testnet.dash.org:3000")
}
