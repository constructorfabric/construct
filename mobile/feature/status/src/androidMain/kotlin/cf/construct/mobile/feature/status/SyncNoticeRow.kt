package cf.construct.mobile.feature.status

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls
import cf.construct.mobile.core.designsystem.Spacing

/**
 * The notice at the top of a page: "Syncing with your computer.", "Up to date. Synced just now." for a few seconds,
 * and "The computer is not reachable" with "Try again". It draws nothing when the sync has nothing to tell, and the
 * others come and go with a short fade. It shows the state it is given and syncs nothing.
 *
 * "Synced just now" is a polite live region, so a screen reader says it once, after what it is saying.
 */
@Composable
fun SyncNoticeRow(
    notice: SyncNotice,
    lastSyncedAt: String?,
    onTryAgain: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // The notice that is leaving keeps its words until it has faded.
    val shown = remember { ShownNotice(notice) }
    if (notice != SyncNotice.None) shown.notice = notice
    AnimatedVisibility(
        visible = notice != SyncNotice.None,
        modifier = modifier,
        enter = fadeIn(tween(SYNC_FADE_MILLIS)) + expandVertically(tween(SYNC_FADE_MILLIS)),
        exit = fadeOut(tween(SYNC_FADE_MILLIS)) + shrinkVertically(tween(SYNC_FADE_MILLIS)),
    ) {
        NoticeContent(shown.notice, lastSyncedAt, onTryAgain)
    }
}

private class ShownNotice(
    var notice: SyncNotice,
)

@Composable
private fun NoticeContent(
    notice: SyncNotice,
    lastSyncedAt: String?,
    onTryAgain: () -> Unit,
) {
    val text =
        when (notice) {
            SyncNotice.Syncing -> stringResource(R.string.sync_syncing)
            SyncNotice.JustSynced -> stringResource(R.string.sync_just_synced)
            else -> notReachableText(lastSyncedAt)
        }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Spacing.L.dp, vertical = Spacing.M.dp)
            .semantics(mergeDescendants = true) {
                if (notice == SyncNotice.JustSynced) liveRegion = LiveRegionMode.Polite
            },
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
    ) {
        NoticeIcon(notice)
        Column {
            Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (notice == SyncNotice.NotReachable) {
                // No padding at the start, so "Try again" lines up with the words above it.
                TextButton(
                    onClick = onTryAgain,
                    modifier = Modifier.heightIn(min = SmallControls.action),
                    contentPadding = PaddingValues(),
                ) {
                    Text(
                        stringResource(R.string.sync_try_again),
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }
        }
    }
}

@Composable
private fun NoticeIcon(notice: SyncNotice) {
    if (notice == SyncNotice.Syncing) {
        CircularProgressIndicator(Modifier.size(SmallControls.icon), strokeWidth = SmallControls.spinnerStroke)
    } else {
        Icon(
            if (notice == SyncNotice.JustSynced) SyncIcons.check else SyncIcons.monitorOff,
            contentDescription = null,
            modifier = Modifier.size(SmallControls.icon),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
