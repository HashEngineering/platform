package org.dash.sdk.usernamesearch

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.dash.sdk.DashSDK
import org.dash.sdk.models.Network

/**
 * Immutable UI state for the username-search screen.
 */
data class SearchUiState(
    val query: String = "",
    val isLoading: Boolean = false,
    val results: List<String> = emptyList(),
    val error: String? = null,
)

/**
 * Owns the [DashSDK] handle and drives DPNS prefix searches.
 *
 * The SDK is created once with [DashSDK.createTrusted] (testnet trusted nodes) and closed in
 * [onCleared]. Query text changes are debounced, then each non-blank prefix is sent to
 * [org.dash.sdk.services.DpnsService.search] off the main thread.
 */
@OptIn(FlowPreview::class)
class UsernameSearchViewModel : ViewModel() {

    private val _uiState = MutableStateFlow(SearchUiState())
    val uiState: StateFlow<SearchUiState> = _uiState.asStateFlow()

    // Drives the debounced search pipeline; mirrors uiState.query.
    private val queryFlow = MutableStateFlow("")

    // Created lazily/asynchronously so SDK init (which may touch the network) never blocks
    // the main thread. Null until ready.
    @Volatile
    private var sdk: DashSDK? = null

    init {
        viewModelScope.launch {
            sdk = withContext(Dispatchers.IO) {
                DashSDK.createTrusted(network = Network.TESTNET)
            }
        }
        viewModelScope.launch {
            queryFlow
                .debounce(300)
                .distinctUntilChanged()
                .collect { prefix -> runSearch(prefix) }
        }
    }

    /** Called from the UI as the user types. */
    fun onQueryChange(text: String) {
        _uiState.update { it.copy(query = text) }
        queryFlow.value = text
    }

    private suspend fun runSearch(prefix: String) {
        val trimmed = prefix.trim()
        if (trimmed.isEmpty()) {
            _uiState.update { it.copy(isLoading = false, results = emptyList(), error = null) }
            return
        }
        val handle = sdk
        if (handle == null) {
            _uiState.update { it.copy(isLoading = false, error = "Connecting to Dash testnet…") }
            return
        }
        _uiState.update { it.copy(isLoading = true, error = null) }
        // DpnsService.search already runs on Dispatchers.IO and never throws (failures → []).
        val matches = handle.dpns.search(prefix = trimmed, limit = 20)
        _uiState.update { it.copy(isLoading = false, results = matches, error = null) }
    }

    override fun onCleared() {
        super.onCleared()
        sdk?.close()
        sdk = null
    }
}
