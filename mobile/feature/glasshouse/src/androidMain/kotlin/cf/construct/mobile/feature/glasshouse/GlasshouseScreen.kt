package cf.construct.mobile.feature.glasshouse

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

private const val REFUSED = "Only http and https addresses open here."
private const val NO_START_ADDRESS = "No start address is set. Type an address to open a page."

/**
 * Says why there is no web page to show: the typed address was refused, the page cannot open (with a
 * Reload button that calls [onReload]), or there is no start address yet. It draws nothing otherwise, and
 * it loads nothing. The web view and the address bar come with the page itself.
 */
@Composable
fun GlasshouseScreen(
    state: GlasshouseState,
    onReload: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val problem = state.problem
    val text =
        when {
            problem != null -> problemText(problem)
            state.addressRefused -> REFUSED
            state.noStartAddress && state.address.isEmpty() -> NO_START_ADDRESS
            else -> null
        }
    if (text != null) {
        Column(
            modifier.fillMaxWidth().padding(Spacing.L.dp),
            verticalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        ) {
            val failed = problem != null || state.addressRefused
            val color = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant
            Text(text, color = color)
            if (problem != null) {
                TextButton(onClick = onReload) { Text("Reload") }
            }
        }
    }
}

private fun problemText(problem: GlasshouseProblem): String =
    when (problem) {
        GlasshouseProblem.NotFound -> "The page cannot open: no site was found at this address."
        GlasshouseProblem.Refused -> "The page cannot open: the site refused the connection."
        GlasshouseProblem.Offline -> "The page cannot open: there is no network connection."
        GlasshouseProblem.Certificate -> "The page cannot open: the site's certificate is not trusted."
        GlasshouseProblem.Crashed -> "The page stopped working."
        GlasshouseProblem.Other -> "The page cannot open."
    }
