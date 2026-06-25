package org.dash.sdk.usernamesearch

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel

/**
 * The single screen: a "Username Search" title bar, a search box, and a prefix-matched
 * list of DPNS usernames returned by [UsernameSearchViewModel].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun UsernameSearchScreen(
    viewModel: UsernameSearchViewModel = viewModel(),
) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()

    Scaffold(
        topBar = { TopAppBar(title = { Text("Username Search") }) }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 16.dp),
        ) {
            OutlinedTextField(
                value = state.query,
                onValueChange = viewModel::onQueryChange,
                label = { Text("Search usernames") },
                placeholder = { Text("e.g. ali") },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 8.dp),
            )

            when {
                state.isLoading -> CenteredBox { CircularProgressIndicator() }

                state.error != null -> CenteredBox {
                    Text(state.error!!, textAlign = TextAlign.Center)
                }

                state.query.isBlank() -> CenteredBox {
                    Text("Type a prefix to search for usernames.", textAlign = TextAlign.Center)
                }

                state.results.isEmpty() -> CenteredBox {
                    Text("No usernames found for \"${state.query}\".", textAlign = TextAlign.Center)
                }

                else -> LazyColumn(modifier = Modifier.fillMaxSize()) {
                    items(state.results) { name ->
                        ListItem(headlineContent = { Text(name) })
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

@Composable
private fun CenteredBox(content: @Composable () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            content()
        }
    }
}
