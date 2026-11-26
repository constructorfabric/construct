package cf.construct.mobile.feature.chat

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.ConversationSummary
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException

@Composable
fun ChatListScreen(
    state: ChatState,
    actions: ChatListActions,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize()) {
        when {
            state.listLoading -> {
                ChatLoading("Loading conversations", Modifier.align(Alignment.Center))
            }

            state.listFailed -> {
                ChatNotice(
                    "The conversations could not be loaded.",
                    modifier = Modifier.padding(Spacing.L.dp),
                    error = true,
                    retry = actions.onRefresh,
                )
            }

            state.conversations.isEmpty() -> {
                EmptyChats(actions.onNew, Modifier.align(Alignment.Center))
            }

            else -> {
                ConversationList(state, actions)
            }
        }
    }
}

@Composable
private fun ConversationList(
    state: ChatState,
    actions: ChatListActions,
) {
    Box(Modifier.fillMaxSize()) {
        LazyColumn(contentPadding = PaddingValues(bottom = chatFabClearance)) {
            items(state.conversations, key = { it.id }) { summary ->
                val active = state.thread?.id == summary.id
                ConversationRow(
                    summary = summary,
                    answering = state.answering && active,
                    enabled = !state.deleting && (!state.answering || active),
                    onClick = { actions.onOpen(summary) },
                )
            }
        }
        if (!state.answering && !state.deleting) {
            FloatingActionButton(
                onClick = actions.onNew,
                modifier = Modifier.align(Alignment.BottomEnd).padding(Spacing.L.dp),
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ) {
                Icon(ChatIcons.plus, contentDescription = "New chat")
            }
        }
        if (state.deleteFailed) {
            ChatNotice(
                "The conversations could not be deleted.",
                modifier = Modifier.align(Alignment.BottomStart).padding(Spacing.L.dp),
                error = true,
            )
        }
    }
}

@Composable
private fun ConversationRow(
    summary: ConversationSummary,
    answering: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val type = MaterialTheme.typography
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = Spacing.L.dp, vertical = Spacing.S.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp),
        ) {
            Text(
                summary.title.ifBlank { "Untitled" },
                modifier = Modifier.weight(1f),
                style = type.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(chatDate(summary.time), style = type.bodySmall, color = scheme.onSurfaceVariant)
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp),
        ) {
            if (answering) {
                Row(
                    Modifier.weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Spacing.XS.dp),
                ) {
                    CircularProgressIndicator(Modifier.size(SmallControls.icon), strokeWidth = chatStroke)
                    Text("Answering", style = type.bodyMedium, fontWeight = FontWeight.SemiBold)
                }
            } else {
                Text(
                    summary.preview.replace('\n', ' '),
                    modifier = Modifier.weight(1f),
                    style = type.bodyMedium,
                    color = scheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            OriginLabel(summary.origin)
        }
    }
}

@Composable
private fun OriginLabel(origin: ConversationOrigin) {
    val phone = origin == ConversationOrigin.PHONE
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Spacing.XS.dp)) {
        Icon(if (phone) ChatIcons.phone else ChatIcons.computer, null, Modifier.size(SmallControls.icon), tint = color)
        Text(if (phone) "This phone" else "Desktop", style = MaterialTheme.typography.bodySmall, color = color)
    }
}

@Composable
private fun EmptyChats(
    onNew: () -> Unit,
    modifier: Modifier,
) {
    Column(
        modifier.padding(Spacing.L.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Spacing.M.dp),
    ) {
        Icon(ChatIcons.message, null, Modifier.size(chatEmptyIcon), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text("No chats yet", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(
            "Start a conversation here and it will show up in this list.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
        )
        OutlinedButton(onClick = onNew, modifier = Modifier.heightIn(min = SmallControls.action)) {
            Icon(ChatIcons.plus, null, Modifier.size(SmallControls.icon))
            Text("New chat", Modifier.padding(start = Spacing.S.dp))
        }
    }
}

private val chatDateFormat = DateTimeFormatter.ofPattern("EEE, MMM d")
private val chatWeekdayFormat = DateTimeFormatter.ofPattern("EEE")

private fun chatDate(time: String): String =
    try {
        val today = LocalDate.now()
        val date = Instant.parse(time).atZone(ZoneId.systemDefault()).toLocalDate()
        when {
            date == today -> "Today"
            date == today.minusDays(1) -> "Yesterday"
            date in today.minusWeeks(1)..today -> date.format(chatWeekdayFormat)
            else -> date.format(chatDateFormat)
        }
    } catch (_: DateTimeParseException) {
        time
    }
