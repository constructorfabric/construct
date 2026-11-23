package cf.construct.mobile.core.model

/** One conversation of the list. */
data class ConversationSummary(
    val id: String,
    /** The first message, cut by the agent; it never holds more than the user typed. */
    val title: String,
    val time: String,
    val origin: ConversationOrigin = ConversationOrigin.COMPUTER,
    val preview: String = "",
)

enum class ConversationOrigin { PHONE, COMPUTER }

enum class ChatRole { USER, ASSISTANT, TOOL }

/**
 * A message of the thread. A [ChatRole.TOOL] message is the short line of one tool call; it never
 * holds the call's arguments or result.
 */
data class ChatMessage(
    val role: ChatRole,
    val text: String,
)

/** What the agent says while it answers. */
sealed interface ChatEvent {
    data class Start(
        val conversation: String,
    ) : ChatEvent

    data class Delta(
        val text: String,
    ) : ChatEvent

    data class Tool(
        val line: String,
    ) : ChatEvent

    data class Done(
        val stopped: Boolean,
    ) : ChatEvent

    data class Error(
        val reason: String,
    ) : ChatEvent
}

data class ChatInput(
    /** An id, or empty for a new conversation. */
    val conversation: String,
    val message: String,
)

/** What `deleteConversation` takes to delete every conversation. */
const val DELETE_ALL_CONVERSATIONS = "all"
