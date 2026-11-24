package cf.construct.mobile.feature.chat

import cf.construct.mobile.core.construct.NO_CHAT_MODEL
import cf.construct.mobile.core.construct.NO_SUCH_CONVERSATION
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.ConversationSummary

data class ChatState(
    val conversations: List<ConversationSummary> = emptyList(),
    val listLoading: Boolean = true,
    val listFailed: Boolean = false,
    val thread: ChatThread? = null,
    val threadOpen: Boolean = false,
    val modelName: String = "",
    val modelReady: Boolean = false,
    val modelReadFailed: Boolean = false,
    val draft: String = "",
    val answering: Boolean = false,
    val stopping: Boolean = false,
    val deleting: Boolean = false,
    val deleteFailed: Boolean = false,
)

data class ChatThread(
    val id: String = "",
    val title: String = "",
    val origin: ConversationOrigin = ConversationOrigin.PHONE,
    val messages: List<ChatMessage> = emptyList(),
    val loading: Boolean = false,
    val loadFailed: Boolean = false,
    val reply: ChatReply? = null,
) {
    val readOnly: Boolean get() = origin == ConversationOrigin.COMPUTER
}

sealed interface ChatReply {
    data object Stopped : ChatReply

    data class Failed(
        val reason: String,
    ) : ChatReply {
        val modelMissing: Boolean get() = reason == NO_CHAT_MODEL
        val hasSettingsFix: Boolean get() = reason in settingsReasons
        val message: String
            get() =
                failureMessages[reason]
                    ?: providerStatus.matchEntire(reason)?.let {
                        "The provider answered with status ${it.groupValues[1]}."
                    }
                    ?: "The answer could not be completed."
    }
}

private val settingsReasons = setOf("key refused", "model not found", "unreachable", NO_CHAT_MODEL)
private val providerStatus = Regex("error ([1-5][0-9]{2})")
private const val BUSY_TEXT = "Another answer is still running."
private const val MISSING_CONVERSATION_TEXT = "This conversation is not there any more."

private val failureMessages =
    mapOf(
        "key refused" to "The provider did not accept the API key.",
        "model not found" to "The provider has no model with this name.",
        "unreachable" to "Nothing answered at the model address.",
        "timeout" to "The model did not answer in time.",
        "busy" to BUSY_TEXT,
        "an answer is already running" to BUSY_TEXT,
        "too long" to "This conversation is too long for the model. Start a new chat.",
        "cut" to "The answer was cut: the connection broke or went silent.",
        "invalid request" to "The agent did not accept the request.",
        "empty message" to "The message is empty.",
        "unknown conversation" to MISSING_CONVERSATION_TEXT,
        "request too large" to "The message is too long to send.",
        "cannot save" to "The answer could not be saved.",
        NO_CHAT_MODEL to "The model settings could not be used.",
        NO_SUCH_CONVERSATION to MISSING_CONVERSATION_TEXT,
    )
