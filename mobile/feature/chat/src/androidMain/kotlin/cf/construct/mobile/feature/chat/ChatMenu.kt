package cf.construct.mobile.feature.chat

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS

@Composable
fun ChatListMenu(
    state: ChatState,
    onDelete: (String) -> Unit,
) {
    ChatMenu(
        enabled =
            !state.answering && !state.deleting && state.conversations.any { it.origin == ConversationOrigin.PHONE },
        all = true,
        onDelete = { onDelete(DELETE_ALL_CONVERSATIONS) },
    )
}

@Composable
internal fun ChatMenu(
    enabled: Boolean,
    all: Boolean,
    onDelete: () -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    var confirm by rememberSaveable { mutableStateOf(false) }
    val label = if (all) "Delete all" else "Delete"
    IconButton(onClick = { expanded = true }, enabled = enabled) {
        Icon(ChatIcons.more, contentDescription = "Conversation options")
    }
    DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
        DropdownMenuItem(
            text = { Text(label) },
            onClick = {
                expanded = false
                confirm = true
            },
        )
    }
    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(if (all) "Delete all conversations?" else "Delete this conversation?") },
            text = {
                Text(
                    if (all) {
                        "Chats started on this phone will be deleted. Chats from a computer will stay."
                    } else {
                        "This removes the conversation and its messages."
                    },
                )
            },
            confirmButton = {
                TextButton(
                    enabled = enabled,
                    onClick = {
                        confirm = false
                        onDelete()
                    },
                ) {
                    Text(if (all) "Yes, delete all" else "Delete", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { confirm = false }) { Text("Cancel") } },
        )
    }
}
