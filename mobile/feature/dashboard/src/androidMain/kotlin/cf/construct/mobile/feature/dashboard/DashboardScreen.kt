package cf.construct.mobile.feature.dashboard

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/**
 * Shows [state] and loads nothing: the failure of a read, the empty page, and the widgets of the document with the
 * total of each. The Android Dashboard page is a later task of the dashboard story, which draws the widgets; this
 * screen only shows that the state holder reaches Android.
 */
@Composable
fun DashboardScreen(
    state: DashboardState,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize().padding(Spacing.L.dp)) {
        val failure = state.failure
        when {
            failure != null -> {
                Text(failure, color = MaterialTheme.colorScheme.error)
            }

            state.loading && !state.hasDashboard -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            }

            state.isEmpty -> {
                Text("Nothing from your computer yet.", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }

            else -> {
                Text(state.title, color = MaterialTheme.colorScheme.onBackground)
                state.widgets.forEach { shown ->
                    val numbers = shown.numbers
                    Text(
                        text = if (numbers == null) "Not sent by your computer" else "${numbers.totalMinutes} min",
                        modifier = Modifier.fillMaxWidth().padding(top = Spacing.S.dp),
                        color = MaterialTheme.colorScheme.onBackground,
                    )
                }
            }
        }
    }
}
