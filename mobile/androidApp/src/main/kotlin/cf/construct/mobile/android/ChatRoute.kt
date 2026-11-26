package cf.construct.mobile.android

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.feature.chat.ChatHolder
import cf.construct.mobile.feature.chat.ChatListActions
import cf.construct.mobile.feature.chat.ChatListMenu
import cf.construct.mobile.feature.chat.ChatListScreen
import cf.construct.mobile.feature.chat.ChatThreadActions
import cf.construct.mobile.feature.chat.ChatThreadScreen
import cf.construct.mobile.feature.status.SyncState

@Composable
internal fun ChatRoute(
    chat: ChatViewModel,
    sync: SyncState,
    onTryAgain: () -> Unit,
    computerContent: @Composable (ComputerPage, (ComputerPage) -> Unit) -> Unit,
    otherPage: @Composable (Page, () -> Unit) -> Unit,
) {
    var selected by rememberSaveable { mutableStateOf(Page.Chat) }
    var fromChat by rememberSaveable { mutableStateOf(false) }
    val state by chat.holder.state.collectAsState()
    val openSettings = {
        fromChat = true
        selected = Page.Settings
    }
    val actions =
        remember(chat) {
            ChatThreadActions(
                onBack = chat.holder::closeThread,
                onChangeDraft = chat.holder::changeDraft,
                onSend = chat.holder::send,
                onStop = chat.holder::stop,
                onRetry = chat.holder::retry,
                onDelete = chat.holder::delete,
                onOpenSettings = openSettings,
            )
        }
    BackHandler(enabled = selected == Page.Settings && fromChat) {
        fromChat = false
        selected = Page.Chat
    }

    val thread = state.thread
    if (selected == Page.Chat && state.threadOpen && thread != null) {
        BackHandler(onBack = actions.onBack)
        ChatThreadScreen(state, thread, actions)
    } else {
        AppFrame(
            sync = sync,
            onTryAgain = onTryAgain,
            selected = selected,
            onSelect = {
                fromChat = false
                selected = it
            },
            pageActions = {
                if (selected == Page.Chat) ChatListMenu(state, chat.holder::delete)
            },
            computerContent = computerContent,
        ) { page, openComputer ->
            if (page == Page.Chat) {
                ChatListScreen(state, chat.listActions)
            } else {
                otherPage(page, openComputer)
            }
        }
    }
}

internal class ChatViewModel(
    construct: Construct,
    settings: ModelSettingsStore,
) : ViewModel() {
    val holder = ChatHolder(construct, settings, viewModelScope)
    val listActions =
        ChatListActions(
            onNew = holder::newChat,
            onOpen = holder::open,
            onRefresh = holder::refresh,
        )
}
