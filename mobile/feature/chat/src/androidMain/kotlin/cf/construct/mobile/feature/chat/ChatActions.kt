package cf.construct.mobile.feature.chat

data class ChatListActions(
    val onNew: () -> Unit,
    val onOpen: (cf.construct.mobile.core.model.ConversationSummary) -> Unit,
    val onRefresh: () -> Unit,
)

data class ChatThreadActions(
    val onBack: () -> Unit,
    val onChangeDraft: (String) -> Unit,
    val onSend: (String) -> Unit,
    val onStop: () -> Unit,
    val onRetry: () -> Unit,
    val onDelete: (String) -> Unit,
    val onOpenSettings: () -> Unit,
)
