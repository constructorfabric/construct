package cf.construct.mobile.core.database

import cf.construct.mobile.core.chat.ChatAnswer
import cf.construct.mobile.core.chat.ChatCoreException
import cf.construct.mobile.core.chat.newChatAnswer
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.NO_SUCH_CONVERSATION
import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** The app's Construct over its copy and the shared Rust chat core. */
class PhoneConstruct private constructor(
    private val store: CopyStore,
    private val settings: ModelSettingsStore,
    private val dispatcher: CoroutineDispatcher,
    private val database: DatabaseConstruct,
) : Construct by database {
    constructor(
        store: CopyStore,
        settings: ModelSettingsStore,
        dispatcher: CoroutineDispatcher,
    ) : this(store, settings, dispatcher, DatabaseConstruct(store, dispatcher))

    private val scope = CoroutineScope(SupervisorJob() + dispatcher)
    private val commands = Mutex()
    private var active: RunningAnswer? = null
    private val events = MutableSharedFlow<ChatEvent>()

    // A native callback cannot suspend. A separate coroutine forwards the lines to the collectors
    // in order, independently of the thread blocked in the native send.
    private val pending = Channel<ChatEvent>(EVENT_BUFFER_CAPACITY)
    private val eventSlots = Semaphore(EVENT_BUFFER_CAPACITY)
    override val chatEvents: Flow<ChatEvent> = events.asSharedFlow()

    init {
        scope.launch {
            for (event in pending) {
                eventSlots.release()
                events.emit(event)
            }
        }
    }

    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> =
        onDisk {
            val origin = store.conversationOrigin(input.conversation)
            when {
                origin != null && origin != PHONE_ORIGIN -> Outcome.Failed(COMPUTER_READ_ONLY)
                active != null -> Outcome.Failed("an answer is already running")
                input.conversation.isNotEmpty() && origin == null -> Outcome.Failed(NO_SUCH_CONVERSATION)
                input.message.isBlank() -> Outcome.Failed("empty message")
                else -> start(input)
            }
        }

    private fun start(input: ChatInput): Outcome<Unit> {
        val model = settings.readProvider()
        if (model.address.isBlank() || model.model.isBlank() || model.key.isNullOrBlank()) {
            queueEvent(ChatEvent.Error(NO_MODEL))
            return Outcome.Failed(NO_MODEL)
        }
        val history = store.chatRecords(input.conversation)
        val request = answerRequest(input, model, history, store.facts())
        val native = newChatAnswer()
        // Reserve room for the terminal event before a native callback can fill the queue.
        return if (!eventSlots.tryAcquire()) {
            native.close()
            Outcome.Failed(EVENT_STREAM_BEHIND)
        } else {
            val answer = RunningAnswer(native)
            active = answer
            scope.launch { runAnswer(answer, request, history.size.toLong()) }
            Outcome.Ok(Unit)
        }
    }

    private fun queueEvent(event: ChatEvent): Boolean {
        if (!eventSlots.tryAcquire()) return false
        check(pending.trySend(event).isSuccess)
        return true
    }

    @Suppress("TooGenericExceptionCaught")
    private suspend fun runAnswer(
        answer: RunningAnswer,
        request: String,
        firstPosition: Long,
    ) {
        var position = firstPosition
        var end: ChatEvent = ChatEvent.Error("the chat core could not answer")
        var overflow = false
        try {
            answer.native.send(
                request,
                writeRecord = { line ->
                    runCatching {
                        store.appendPhoneRecord(PhoneRecord.read(line), position, line)
                        position++
                    }.isSuccess
                },
                emitEvent = { line ->
                    runCatching {
                        when (val event = chatEvent(line)) {
                            is ChatEvent.Done, is ChatEvent.Error -> {
                                end = event
                                true
                            }

                            else -> {
                                val queued = queueEvent(event)
                                overflow = !queued
                                queued
                            }
                        }
                    }.getOrDefault(false)
                },
            )
        } catch (_: Exception) {
            end = ChatEvent.Error("the chat core could not answer")
        } finally {
            answer.finished.complete(Unit)
            commands.withLock {
                answer.native.close()
                active = null
                if (overflow) end = ChatEvent.Error(EVENT_STREAM_BEHIND)
                check(pending.trySend(end).isSuccess)
            }
        }
    }

    override suspend fun stopAnswer() {
        commands.withLock { active?.native?.stop() }
    }

    override suspend fun deleteConversation(which: String): Outcome<Unit> =
        onDisk {
            val origin = store.conversationOrigin(which)
            when {
                origin != null && origin != PHONE_ORIGIN -> Outcome.Failed(COMPUTER_READ_ONLY)
                active != null -> Outcome.Failed("busy")
                else -> store.deletePhoneConversations(which)
            }
        }

    override suspend fun eraseEverything(): Outcome<EraseStarted> =
        onDisk {
            active?.let { answer ->
                answer.native.stop()
                answer.finished.await()
            }
            database.eraseEverything()
        }

    @Suppress("TooGenericExceptionCaught")
    private suspend fun <T> onDisk(block: suspend () -> Outcome<T>): Outcome<T> =
        withContext(dispatcher) {
            commands.withLock {
                try {
                    block()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: ModelSettingsException) {
                    Outcome.Failed(failure.message ?: NO_MODEL)
                } catch (failure: ChatCoreException) {
                    Outcome.Failed(failure.message ?: "the chat core could not load")
                } catch (_: Exception) {
                    Outcome.Failed(DatabaseConstruct.UNREADABLE)
                }
            }
        }

    private class RunningAnswer(
        val native: ChatAnswer,
        val finished: CompletableDeferred<Unit> = CompletableDeferred(),
    )

    private companion object {
        const val EVENT_BUFFER_CAPACITY = 64
        const val EVENT_STREAM_BEHIND = "the chat event stream fell behind"
    }
}
