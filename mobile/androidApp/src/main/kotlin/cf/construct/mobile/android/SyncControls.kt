package cf.construct.mobile.android

import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/** "Sync now", which says "Syncing…" with a spinner, and cannot be pressed again, while a sync is under way. */
@Composable
internal fun SyncNowButton(
    syncing: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val sizes = LocalControlSizes.current
    WideButton(onClick = onClick, enabled = enabled && !syncing) {
        if (syncing) {
            CircularProgressIndicator(Modifier.size(sizes.noticeIcon), strokeWidth = sizes.spinnerStroke)
            Spacer(Modifier.width(Spacing.S.dp))
            Text("Syncing…")
        } else {
            Text("Sync now")
        }
    }
}

@Composable
internal fun WideButton(
    onClick: () -> Unit,
    enabled: Boolean,
    content: @Composable RowScope.() -> Unit,
) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth().heightIn(min = LocalControlSizes.current.outlinedButton),
        content = content,
    )
}
