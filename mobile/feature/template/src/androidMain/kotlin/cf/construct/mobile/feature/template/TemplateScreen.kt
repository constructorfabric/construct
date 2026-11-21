package cf.construct.mobile.feature.template

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/** Shows [state] and loads nothing. */
@Composable
fun TemplateScreen(
    state: TemplateState,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        when {
            state.failure != null -> {
                Text(state.failure, color = MaterialTheme.colorScheme.error)
            }

            state.loading && state.items.isEmpty() -> {
                CircularProgressIndicator()
            }

            else -> {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(state.items) { item ->
                        Text(item, Modifier.padding(Spacing.L.dp), color = MaterialTheme.colorScheme.onBackground)
                    }
                }
            }
        }
    }
}
