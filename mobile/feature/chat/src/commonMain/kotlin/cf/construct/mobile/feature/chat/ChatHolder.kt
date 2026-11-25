package cf.construct.mobile.feature.chat

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A chat session. Call its actions on the main thread, where its scope also runs. */
@Suppress("TooManyFunctions")
class ChatHolder(
    private val construct: Construct,
    private val settings: ModelSettingsStore,
    private val scope: CoroutineScope,
    private val storageDispatcher: CoroutineDispatcher = Dispatchers.Default,
) {
    private val mutable = MutableStateFlow(ChatState())
    val state = mutable.asStateFlow()
    private var listRead: Job? = null
    private var threadRead: Job? = null
    private var modelRead: Job? = null
    private var settingsRevision = 0L
    private var sendSettingsRevision = 0L
    private var sendJob: Job? = null
    private var attempt: ChatInput? = null
    private var retryText = ""
    private var started = false
    private var appendedUserTurn = false
    private var messagesAtSend = emptyList<ChatMessage>()

    init {
        scope.launch(start = CoroutineStart.UNDISPATCHED) { construct.chatEvents.collect(::onEvent) }
        // A sync changes the list under a page that is open: the list is read again on each signal. The first
        // signal comes at once, and is the read that cannot miss a change made while the first read ran.
        scope.launch { construct.conversationChanges.collect { refreshList() } }
        refresh()
    }

    fun refresh() {
        refreshList()
        readModelSettings()
    }

    /** Refresh the model after Settings commits a change. */
    fun refreshModel() {
        settingsRevision++
        readModelSettings()
    }

    private fun readModelSettings() {
        modelRead?.cancel()
        modelRead =
            scope.launch {
                try {
                    val model = withContext(storageDispatcher) { settings.read() }
                    mutable.update {
                        it.copy(
                            modelName = model.model,
                            modelReady = model.address.isNotBlank() && model.model.isNotBlank() && model.keyIsSet,
                            modelReadFailed = false,
                        )
                    }
                } catch (_: ModelSettingsException) {
                    mutable.update { it.copy(modelName = "", modelReady = false, modelReadFailed = true) }
                }
            }
    }

    fun newChat() {
        if (state.value.answering || state.value.deleting) return
        show(ChatThread())
    }

    fun open(summary: ConversationSummary) {
        val current = state.value
        if (current.deleting) return
        if (current.thread?.id == summary.id && !current.thread.loadFailed) {
            mutable.update { it.copy(threadOpen = true) }
        } else if (!current.answering) {
            loadThread(summary)
        }
    }

    private fun loadThread(summary: ConversationSummary) {
        show(ChatThread(summary.id, summary.title, summary.origin, loading = true))
        threadRead =
            scope.launch {
                when (val result = construct.getConversation(summary.id)) {
                    is Outcome.Ok -> updateThread { it.copy(loading = false, messages = result.value) }
                    is Outcome.Failed -> updateThread { it.copy(loading = false, loadFailed = true) }
                }
            }
    }

    fun closeThread() {
        mutable.update { it.copy(threadOpen = false, deleteFailed = false) }
        refreshList()
    }

    fun changeDraft(text: String) {
        mutable.update { it.copy(draft = text) }
    }

    fun send(text: String = state.value.draft) {
        val current = state.value
        val thread = current.thread ?: return
        val message = text.trim()
        if (message.isEmpty() || !current.canSend(thread)) return

        val request = ChatInput(thread.id, message)
        attempt = request
        retryText = message
        started = false
        // The core reuses a matching unanswered user turn, including when tools ran after it.
        val lastTurn = thread.messages.lastOrNull { it.role != ChatRole.TOOL }
        appendedUserTurn = lastTurn?.role != ChatRole.USER || lastTurn.text != message
        val messages = if (appendedUserTurn) thread.messages + ChatMessage(ChatRole.USER, message) else thread.messages
        messagesAtSend = messages
        mutable.update {
            it.copy(
                draft = if (text == it.draft) "" else it.draft,
                answering = true,
                thread = thread.copy(messages = messages, reply = null),
            )
        }
        sendJob =
            scope.launch {
                sendSettingsRevision = settingsRevision
                val result = construct.sendMessage(request)
                if (attempt === request && result is Outcome.Failed) finish(ChatReply.Failed(result.error))
            }
    }

    fun stop() {
        val request = attempt ?: return
        if (state.value.stopping) return
        mutable.update { it.copy(stopping = true) }
        scope.launch {
            sendJob?.join()
            if (attempt === request) construct.stopAnswer()
        }
    }

    fun retry() {
        if (state.value.thread?.reply != null) send(retryText)
    }

    fun delete(which: String) {
        val current = state.value
        if (current.answering || current.deleting) return
        val canDelete =
            if (which == DELETE_ALL_CONVERSATIONS) {
                current.conversations.any { it.origin == ConversationOrigin.PHONE }
            } else {
                current.thread?.let { it.id == which && !it.readOnly } == true && which.isNotEmpty()
            }
        if (!canDelete) return
        mutable.update { it.copy(deleting = true, deleteFailed = false) }
        scope.launch {
            when (construct.deleteConversation(which)) {
                is Outcome.Ok -> {
                    listRead?.cancel()
                    threadRead?.cancel()
                    mutable.update { it.copy(thread = null, threadOpen = false, draft = "") }
                    loadList()
                    mutable.update { it.copy(deleting = false) }
                }

                is Outcome.Failed -> {
                    mutable.update { it.copy(deleting = false, deleteFailed = true) }
                }
            }
        }
    }

    private fun show(thread: ChatThread) {
        threadRead?.cancel()
        retryText = ""
        mutable.update { it.copy(thread = thread, threadOpen = true, draft = "", deleteFailed = false) }
    }

    private fun refreshList() {
        listRead?.cancel()
        listRead = scope.launch { loadList() }
    }

    private suspend fun loadList() {
        when (val result = construct.listConversations()) {
            is Outcome.Ok -> {
                mutable.update {
                    it.copy(conversations = result.value, listLoading = false, listFailed = false)
                }
            }

            is Outcome.Failed -> {
                mutable.update { it.copy(listLoading = false, listFailed = true) }
            }
        }
    }

    private suspend fun onEvent(event: ChatEvent) {
        val request = attempt ?: return
        when (event) {
            is ChatEvent.Start -> {
                started = true
                updateThread { it.copy(id = event.conversation, title = it.title.ifEmpty { retryText }) }
                refreshList()
            }

            is ChatEvent.Delta -> {
                updateThread { it.copy(messages = it.messages.withDelta(event.text)) }
            }

            is ChatEvent.Tool -> {
                updateThread { it.copy(messages = it.messages + ChatMessage(ChatRole.TOOL, event.line)) }
            }

            is ChatEvent.Done -> {
                finish(if (event.stopped) ChatReply.Stopped else null)
            }

            is ChatEvent.Error -> {
                val messages = if (event.reason == "cannot save" && started) savedMessages() else null
                if (attempt === request) finish(ChatReply.Failed(event.reason), messages)
            }
        }
    }

    private suspend fun savedMessages(): List<ChatMessage> {
        // Start confirms the user turn. Keep it and the earlier history if the copy cannot be read.
        val beforeAnswer = messagesAtSend
        val id = state.value.thread?.id ?: return beforeAnswer
        return when (val saved = construct.getConversation(id)) {
            is Outcome.Ok -> saved.value
            is Outcome.Failed -> beforeAnswer
        }
    }

    private fun finish(
        reply: ChatReply?,
        savedMessages: List<ChatMessage>? = null,
    ) {
        val discardOptimisticTurn = !started && appendedUserTurn
        val missingFromCurrentSettings =
            (reply as? ChatReply.Failed)?.modelMissing == true && sendSettingsRevision == settingsRevision
        if (missingFromCurrentSettings) modelRead?.cancel()
        attempt = null
        messagesAtSend = emptyList()
        mutable.update { current ->
            current.copy(
                answering = false,
                stopping = false,
                modelReady = current.modelReady && !missingFromCurrentSettings,
                thread =
                    current.thread?.let { thread ->
                        thread.copy(
                            messages =
                                when {
                                    savedMessages != null -> savedMessages
                                    discardOptimisticTurn -> thread.messages.dropLast(1)
                                    else -> thread.messages
                                },
                            reply = reply,
                        )
                    },
            )
        }
        refreshList()
    }

    private fun updateThread(transform: (ChatThread) -> ChatThread) {
        mutable.update { it.copy(thread = it.thread?.let(transform)) }
    }
}

private fun ChatState.canSend(thread: ChatThread): Boolean =
    threadOpen && modelReady && !answering && !deleting && !thread.readOnly && !thread.loading && !thread.loadFailed

private fun List<ChatMessage>.withDelta(delta: String): List<ChatMessage> {
    val last = lastOrNull()
    return if (last?.role == ChatRole.ASSISTANT) {
        dropLast(1) + last.copy(text = last.text + delta)
    } else {
        this + ChatMessage(ChatRole.ASSISTANT, delta)
    }
}
