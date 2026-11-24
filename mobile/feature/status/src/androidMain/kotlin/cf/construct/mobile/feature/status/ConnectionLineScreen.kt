package cf.construct.mobile.feature.status

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

@Composable
fun ConnectionLine(
    state: ConnectionStatus,
    onTryAgain: () -> Unit,
    dotSize: Dp,
    modifier: Modifier = Modifier,
) {
    val dot = state.kind.dotColor()
    Row(
        modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(horizontal = Spacing.L.dp, vertical = Spacing.S.dp),
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(dotSize).background(dot, CircleShape))
        Column(Modifier.weight(1f)) {
            ConnectionText(state.kind.title)
            state.kind.detail?.let { ConnectionText(it) }
            if (state.kind.detail != null) lastContactText(state.lastContactMillis)?.let { ConnectionText(it) }
        }
        if (state.kind.canRetry) {
            TextButton(onClick = onTryAgain) { Text(stringResource(R.string.connection_try_again)) }
        }
    }
}

@Composable
private fun ConnectionText(text: String) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
}
