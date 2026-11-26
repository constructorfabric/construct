package cf.construct.mobile.feature.chat

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole

@Composable
fun ChatThreadScreen(
    state: ChatState,
    thread: ChatThread,
    actions: ChatThreadActions,
    modifier: Modifier = Modifier,
) {
    Scaffold(
        modifier = modifier,
        topBar = { ThreadBar(state, thread, actions) },
        bottomBar = { ThreadFooter(state, thread, actions) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when {
                thread.loading -> {
                    ChatLoading("Loading the conversation", Modifier.align(Alignment.Center))
                }

                thread.loadFailed -> {
                    ChatNotice(
                        "The conversation cannot be shown: the connection was lost.",
                        modifier = Modifier.padding(Spacing.L.dp),
                        error = true,
                    )
                }

                thread.messages.isEmpty() && thread.reply == null && !thread.readOnly -> {
                    NewThread(
                        canSend = state.modelReady,
                        onSend = actions.onSend,
                        modifier = Modifier.align(Alignment.Center),
                    )
                }

                else -> {
                    key(thread.id) { ThreadMessages(state, thread, actions) }
                }
            }
        }
    }
}

@Composable
private fun ThreadBar(
    state: ChatState,
    thread: ChatThread,
    actions: ChatThreadActions,
) {
    TopAppBar(
        title = {
            Text(thread.title.ifBlank { "New chat" }, maxLines = 1, overflow = TextOverflow.Ellipsis)
        },
        navigationIcon = {
            IconButton(onClick = actions.onBack) { Icon(ChatIcons.back, contentDescription = "Back to chats") }
        },
        actions = {
            if (!thread.readOnly && thread.id.isNotEmpty() && !thread.loadFailed) {
                ChatMenu(
                    enabled = !state.answering && !state.deleting && !thread.loading,
                    all = false,
                    onDelete = { actions.onDelete(thread.id) },
                )
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface),
    )
}

@Composable
private fun ThreadFooter(
    state: ChatState,
    thread: ChatThread,
    actions: ChatThreadActions,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .imePadding()
            .padding(horizontal = Spacing.L.dp),
    ) {
        when {
            thread.readOnly -> {
                ChatNotice(
                    "This chat was started on a computer and is read-only.",
                    modifier = Modifier.padding(vertical = Spacing.M.dp),
                    icon = ChatIcons.computer,
                )
            }

            else -> {
                if (!state.modelReady && !thread.loading && !thread.loadFailed) {
                    val notice = if (state.modelReadFailed) "The model settings could not be read." else NO_MODEL_TEXT
                    ChatNotice(
                        text = notice,
                        error = state.modelReadFailed,
                        openSettings = actions.onOpenSettings,
                        modifier = Modifier.padding(bottom = Spacing.XS.dp),
                    )
                }
                ChatComposer(state, thread, actions)
            }
        }
    }
}

@Composable
private fun NewThread(
    canSend: Boolean,
    onSend: (String) -> Unit,
    modifier: Modifier,
) {
    Column(
        modifier.padding(Spacing.L.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Spacing.S.dp),
    ) {
        Text(
            "What can I help with?",
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(bottom = Spacing.S.dp),
        )
        for (example in listOf("What do you know about me?", "I mostly write Rust")) {
            OutlinedButton(
                onClick = { onSend(example) },
                enabled = canSend,
                modifier = Modifier.heightIn(min = SmallControls.action),
            ) { Text(example) }
        }
    }
}

@Composable
private fun ThreadMessages(
    state: ChatState,
    thread: ChatThread,
    actions: ChatThreadActions,
) {
    val list = rememberLazyListState()
    val messages = thread.messages
    val newestTool = messages.indexOfLast { it.role == ChatRole.TOOL }
    val reply = thread.reply.takeUnless { (it as? ChatReply.Failed)?.modelMissing == true && !state.modelReady }
    val newestRow =
        when {
            state.deleteFailed -> "delete"
            reply != null -> "reply"
            messages.isNotEmpty() -> messages.lastIndex
            else -> null
        }
    var previousNewestRow by remember { mutableStateOf(newestRow) }
    SideEffect {
        // Override the previous row's key for the next layout only if the reader was at the newest end.
        val wasAtEnd = !list.canScrollBackward && !list.isScrollInProgress
        if (wasAtEnd && newestRow != null && newestRow != previousNewestRow) {
            list.requestScrollToItem(0)
        }
        previousNewestRow = newestRow
    }
    SelectionContainer {
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = list,
            reverseLayout = true,
            verticalArrangement = Arrangement.spacedBy(Spacing.L.dp, Alignment.Top),
            contentPadding = PaddingValues(Spacing.L.dp),
        ) {
            if (state.deleteFailed) {
                item("delete") { ChatNotice("The conversation could not be deleted.", error = true) }
            }
            if (reply != null) {
                item("reply") { ReplyNotice(reply, state.modelReady, actions) }
            }
            itemsIndexed(messages.asReversed(), key = { index, _ -> messages.lastIndex - index }) { index, message ->
                ThreadMessage(message, emphasized = state.answering && messages.lastIndex - index == newestTool)
            }
        }
    }
}

@Composable
private fun ThreadMessage(
    message: ChatMessage,
    emphasized: Boolean,
) {
    val scheme = MaterialTheme.colorScheme
    when (message.role) {
        ChatRole.USER -> {
            BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                Text(
                    message.text,
                    modifier = Modifier.widthIn(max = maxWidth * USER_BUBBLE_FRACTION).wrapUserBubble(),
                    style = MaterialTheme.typography.bodyLarge,
                    color = scheme.onBackground,
                )
            }
        }

        ChatRole.ASSISTANT -> {
            MarkdownAnswer(message.text)
        }

        ChatRole.TOOL -> {
            ChatNotice(message.text, icon = ChatIcons.tool, emphasized = emphasized)
        }
    }
}

private const val USER_BUBBLE_FRACTION = 0.82f

@Composable
private fun Modifier.wrapUserBubble(): Modifier =
    this
        .border(chatStroke, MaterialTheme.colorScheme.outline, MaterialTheme.shapes.large)
        .padding(horizontal = Spacing.L.dp, vertical = Spacing.M.dp)
