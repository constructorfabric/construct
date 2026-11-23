package cf.construct.mobile.feature.chat

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

internal const val NO_MODEL_TEXT = "No model is set yet. Set one in Settings to chat."

@Composable
internal fun ChatLoading(
    text: String,
    modifier: Modifier,
) {
    Column(
        modifier,
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Spacing.M.dp),
    ) {
        CircularProgressIndicator(Modifier.size(Spacing.XL.dp))
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
internal fun ChatNotice(
    text: String,
    modifier: Modifier = Modifier,
    error: Boolean = false,
    icon: ImageVector = if (error) ChatIcons.alert else ChatIcons.info,
    emphasized: Boolean = false,
    openSettings: (() -> Unit)? = null,
    retry: (() -> Unit)? = null,
) {
    val scheme = MaterialTheme.colorScheme
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
    ) {
        Icon(
            icon,
            contentDescription = null,
            modifier = Modifier.size(chatSmallIcon),
            tint = if (error) scheme.error else scheme.onSurfaceVariant,
        )
        Column {
            Text(
                text,
                style = MaterialTheme.typography.bodyMedium,
                color =
                    when {
                        error -> scheme.error
                        emphasized -> scheme.onBackground
                        else -> scheme.onSurfaceVariant
                    },
                fontWeight = if (emphasized) FontWeight.SemiBold else FontWeight.Normal,
            )
            if (openSettings != null || retry != null) {
                FlowRow {
                    if (openSettings != null) NoticeButton("Open Settings", openSettings)
                    if (retry != null) NoticeButton("Try again", retry)
                }
            }
        }
    }
}

@Composable
private fun NoticeButton(
    label: String,
    onClick: () -> Unit,
) {
    TextButton(onClick = onClick, modifier = Modifier.heightIn(min = chatActionHeight)) {
        Text(label, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
internal fun ReplyNotice(
    reply: ChatReply,
    modelReady: Boolean,
    actions: ChatThreadActions,
) {
    val retry = actions.onRetry.takeIf { modelReady }
    when (reply) {
        ChatReply.Stopped -> {
            ChatNotice("Stopped. The part that was written is kept.", retry = retry)
        }

        is ChatReply.Failed -> {
            ChatNotice(
                reply.message,
                error = true,
                openSettings = actions.onOpenSettings.takeIf { reply.hasSettingsFix },
                retry = retry,
            )
        }
    }
}
