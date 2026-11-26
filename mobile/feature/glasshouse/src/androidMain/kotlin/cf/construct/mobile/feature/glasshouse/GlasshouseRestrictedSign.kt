package cf.construct.mobile.feature.glasshouse

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing
import kotlinx.coroutines.delay
import kotlin.time.Duration.Companion.seconds

private val iconSize = 12.dp
private val blockedShown = 6.seconds

/** The small sign that the page is in restricted mode, which nothing switches off. */
@Composable
internal fun RestrictedSign(modifier: Modifier = Modifier) {
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    Row(
        modifier.semantics(mergeDescendants = true) {}.testTag(RESTRICTED_TAG),
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(GlasshouseIcons.lock, contentDescription = null, modifier = Modifier.size(iconSize), tint = color)
        Text("Restricted mode", style = MaterialTheme.typography.labelSmall, color = color)
    }
}

/** The line that says what restricted mode refused, for six seconds after each refusal; nothing when there is none. */
@Composable
internal fun BlockedLine(
    restriction: GlasshouseRestriction,
    modifier: Modifier = Modifier,
) {
    val blocked by restriction.blocked.collectAsState()
    blocked?.let { shown ->
        LaunchedEffect(shown) {
            delay(blockedShown)
            restriction.dismiss(shown)
        }
        GlasshouseMessage(blockedText(shown.action), modifier, textTag = BLOCKED_TAG)
    }
}

private fun blockedText(action: GlasshouseBlockedAction): String =
    when (action) {
        GlasshouseBlockedAction.Download -> "Download is blocked in restricted mode."
        GlasshouseBlockedAction.Print -> "Print is blocked in restricted mode."
        GlasshouseBlockedAction.Share -> "Share is blocked in restricted mode."
    }

/** The tag of the restricted mode sign, for tests. */
const val RESTRICTED_TAG = "glasshouse-restricted"
