package cf.construct.mobile.feature.glasshouse

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

private val iconSize = 16.dp

// The icon sits on the middle of the first line of the text, which is 20 high.
private val iconOffset = 2.dp
private val actionHeight = 44.dp

/**
 * A message of the page, as in the Chat page: an icon and plain text with no box, and under the text a text
 * button when there is something to do. An [error] has the alert icon and the error colour, a notice the info icon.
 */
@Composable
internal fun GlasshouseMessage(
    text: String,
    modifier: Modifier = Modifier,
    error: Boolean = false,
    onReload: (() -> Unit)? = null,
    textTag: String = MESSAGE_TAG,
) {
    val scheme = MaterialTheme.colorScheme
    val color = if (error) scheme.error else scheme.onSurfaceVariant
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp)) {
        Icon(
            if (error) GlasshouseIcons.alert else GlasshouseIcons.info,
            contentDescription = null,
            modifier = Modifier.padding(top = iconOffset).size(iconSize),
            tint = color,
        )
        Column {
            Text(
                text,
                style = MaterialTheme.typography.bodyMedium,
                color = color,
                modifier = Modifier.testTag(textTag),
            )
            if (onReload != null) {
                TextButton(onClick = onReload, modifier = Modifier.heightIn(min = actionHeight).testTag(RELOAD_TAG)) {
                    Text("Reload", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

/** The tag of the message text, for tests. */
const val MESSAGE_TAG = "glasshouse-message"

/** The tag of the line that says restricted mode refused something, for tests. */
const val BLOCKED_TAG = "glasshouse-blocked"

/** The tag of the Reload button of a message, for tests. */
const val RELOAD_TAG = "glasshouse-message-reload"
