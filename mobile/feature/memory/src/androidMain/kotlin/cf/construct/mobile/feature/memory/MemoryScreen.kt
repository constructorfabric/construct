package cf.construct.mobile.feature.memory

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing
import kotlinx.serialization.json.JsonPrimitive

/**
 * Shows [state] and loads nothing. A failed read shows its reason above what is already on screen,
 * with a button that calls [onRefresh].
 */
@Composable
fun MemoryScreen(
    state: MemoryState,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize()) {
        if (state.failure != null) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Spacing.L.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(state.failure, Modifier.weight(1f), color = MaterialTheme.colorScheme.error)
                TextButton(onClick = onRefresh) { Text("Try again") }
            }
        }
        if (state.loading && state.facts.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        } else {
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
