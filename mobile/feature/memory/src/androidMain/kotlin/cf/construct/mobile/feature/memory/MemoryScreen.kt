package cf.construct.mobile.feature.memory

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
import kotlinx.serialization.json.JsonPrimitive

/** Shows [state] and loads nothing. */
@Composable
fun MemoryScreen(
    state: MemoryState,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        when {
            state.failure != null -> {
                Text(state.failure, color = MaterialTheme.colorScheme.error)
            }

            state.loading && state.facts.isEmpty() -> {
                CircularProgressIndicator()
            }

            else -> {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(state.facts, key = { it.factId }) { fact ->
                        Column(Modifier.fillMaxWidth().padding(Spacing.L.dp)) {
                            Text(fact.property, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            val value = fact.value
                            Text(
                                if (value is JsonPrimitive && value.isString) value.content else value.toString(),
                                color = MaterialTheme.colorScheme.onBackground,
                            )
                        }
                    }
                }
            }
        }
    }
}
