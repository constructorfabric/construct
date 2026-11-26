package cf.construct.mobile.feature.chat

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LocalMinimumInteractiveComponentSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls
import cf.construct.mobile.core.designsystem.Spacing

private val composerHeight = 54.dp
private val composerCircle = 36.dp
private const val MAX_DRAFT_LINES = 5

@Composable
internal fun ChatComposer(
    state: ChatState,
    thread: ChatThread,
    actions: ChatThreadActions,
) {
    val scheme = MaterialTheme.colorScheme
    val focus = LocalFocusManager.current
    val editable = !thread.loading && !thread.loadFailed && !state.deleting
    Column(verticalArrangement = Arrangement.spacedBy(Spacing.XS.dp)) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = composerHeight)
                .border(chatStroke, scheme.outline, MaterialTheme.shapes.large)
                .padding(start = Spacing.L.dp, end = Spacing.XS.dp, top = Spacing.XS.dp, bottom = Spacing.XS.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicTextField(
                value = state.draft,
                onValueChange = actions.onChangeDraft,
                enabled = editable,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onBackground),
                cursorBrush = SolidColor(scheme.primary),
                maxLines = MAX_DRAFT_LINES,
                modifier = Modifier.weight(1f).semantics { contentDescription = "Message Construct" },
                decorationBox = { field ->
                    Box {
                        if (state.draft.isEmpty()) {
                            Text(
                                "Message Construct",
                                color = scheme.onSurfaceVariant,
                                style = MaterialTheme.typography.bodyLarge,
                            )
                        }
                        field()
                    }
                },
            )
            ComposerButtons(
                answering = state.answering,
                canSend = editable && state.modelReady && state.draft.isNotBlank() && !state.answering,
                canStop = !state.stopping,
                onSend = {
                    focus.clearFocus()
                    actions.onSend(state.draft)
                },
                onStop = actions.onStop,
            )
        }
        Text(
            if (state.modelName.isBlank()) "Model: not set" else "Model: ${state.modelName}",
            modifier = Modifier.fillMaxWidth().padding(bottom = Spacing.XS.dp),
            textAlign = TextAlign.Center,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.bodySmall,
            color = scheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ComposerButtons(
    answering: Boolean,
    canSend: Boolean,
    canStop: Boolean,
    onSend: () -> Unit,
    onStop: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val button = Modifier.minimumInteractiveComponentSize().size(composerCircle)
    CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides SmallControls.action) {
        if (answering) {
            OutlinedIconButton(onClick = onStop, enabled = canStop, modifier = button, shape = CircleShape) {
                Icon(ChatIcons.stop, contentDescription = "Stop", modifier = Modifier.size(SmallControls.icon))
            }
        }
        if (canSend) {
            FilledIconButton(onClick = onSend, modifier = button, shape = CircleShape) {
                Icon(ChatIcons.send, contentDescription = "Send", modifier = Modifier.size(chatSendIcon))
            }
        } else {
            OutlinedIconButton(
                onClick = onSend,
                enabled = false,
                modifier = button,
                shape = CircleShape,
                colors = IconButtonDefaults.outlinedIconButtonColors(disabledContentColor = scheme.onSurfaceVariant),
            ) {
                Icon(ChatIcons.send, contentDescription = "Send", modifier = Modifier.size(chatSendIcon))
            }
        }
    }
}
