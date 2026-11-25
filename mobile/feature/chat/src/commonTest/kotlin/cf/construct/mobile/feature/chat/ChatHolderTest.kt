package cf.construct.mobile.feature.chat

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.construct.NO_CHAT_MODEL
import cf.construct.mobile.core.keys.FakeModelSettingsStore
import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.valueOrNull
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class ChatHolderTest {
    @Test
    fun stoppingKeepsTheStreamedTextAndRetryUsesTheSameThread() =
        runTest {
            val holder = holder(FakeConstruct(backgroundScope, deltaDelayMillis = 100))
            holder.newChat()
            holder.changeDraft("A new question")
            holder.send()
            runCurrent()
            advanceTimeBy(101)
            holder.stop()
            runCurrent()

            val thread = assertNotNull(holder.state.value.thread)
            assertEquals(ChatReply.Stopped, thread.reply)
            assertEquals(ChatMessage(ChatRole.ASSISTANT, "Here is "), thread.messages.last())
            assertTrue(thread.messages.any { it.role == ChatRole.TOOL })
            assertFalse(holder.state.value.answering)

            holder.retry()
            runCurrent()
            assertEquals(
                thread.id,
                holder.state.value.thread
                    ?.id,
            )
            assertTrue(holder.state.value.answering)
            val retriedMessage =
                holder.state.value.thread
                    ?.messages
                    ?.lastOrNull { it.role == ChatRole.USER }
            assertEquals(ChatMessage(ChatRole.USER, "A new question"), retriedMessage)
            val messages = assertNotNull(holder.state.value.thread).messages
            assertEquals(2, messages.count { it.role == ChatRole.USER })
        }

    @Test
    fun theListThatIsOpenShowsWhatASyncChangesWithoutReopeningIt() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val changes = MutableSharedFlow<Unit>()
            var listed = fake.listConversations().valueOrNull().orEmpty()
            val construct =
                object : Construct by fake {
                    override val conversationChanges = changes

                    override suspend fun listConversations(): Outcome<List<ConversationSummary>> = Outcome.Ok(listed)
                }
            val holder = holder(construct)
            runCurrent()
            assertEquals(
                listOf("c1", "c2"),
                holder.state.value.conversations
                    .map { it.id },
            )

            // A sync brings a new chat of the computer and removes another.
            listed = listOf(ConversationSummary("c3", "From the computer", "2026-11-09T10:00:00Z"), listed.first())
            changes.emit(Unit)
            runCurrent()

            assertEquals(
                listOf("c3", "c1"),
                holder.state.value.conversations
                    .map { it.id },
            )
        }

    @Test
    fun retryReusesThePersistedUnansweredTurnEvenAfterAToolLine() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val events = MutableSharedFlow<ChatEvent>()
            val construct =
                object : Construct by fake {
                    override val chatEvents = events
                    private var sends = 0

                    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
                        if (sends++ == 1) return Outcome.Failed("unreachable")
                        events.emit(ChatEvent.Start("phone-chat"))
                        if (sends == 1) {
                            events.emit(ChatEvent.Tool("Looked up a note"))
                            events.emit(ChatEvent.Error("key refused"))
                        } else {
                            events.emit(ChatEvent.Delta("An answer"))
                            events.emit(ChatEvent.Done(stopped = false))
                        }
                        return Outcome.Ok(Unit)
                    }
                }
            val holder = holder(construct)
            holder.newChat()
            holder.send("Hello")
            runCurrent()
            val unanswered = assertNotNull(holder.state.value.thread).messages
            assertEquals(listOf(ChatRole.USER, ChatRole.TOOL), unanswered.map { it.role })

            holder.retry()
            runCurrent()
            val failedThread = assertNotNull(holder.state.value.thread)
            assertEquals(unanswered, failedThread.messages)
            assertEquals(ChatReply.Failed("unreachable"), failedThread.reply)

            holder.retry()
            runCurrent()
            val answeredThread = assertNotNull(holder.state.value.thread)
            assertEquals(unanswered + ChatMessage(ChatRole.ASSISTANT, "An answer"), answeredThread.messages)
        }

    @Test
    fun retryAfterAnAnswerSaveFailureUsesPersistedHistory() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val events = MutableSharedFlow<ChatEvent>()
            val user = ChatMessage(ChatRole.USER, "Hello")
            val construct =
                object : Construct by fake {
                    override val chatEvents = events
                    private var sends = 0

                    override suspend fun getConversation(conversation: String): Outcome<List<ChatMessage>> {
                        assertEquals("phone-chat", conversation)
                        return Outcome.Ok(listOf(user))
                    }

                    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
                        events.emit(ChatEvent.Start("phone-chat"))
                        if (sends++ == 0) {
                            events.emit(ChatEvent.Delta("Unsaved answer"))
                            events.emit(ChatEvent.Error("cannot save"))
                        } else {
                            events.emit(ChatEvent.Delta("Saved answer"))
                            events.emit(ChatEvent.Done(stopped = false))
                        }
                        return Outcome.Ok(Unit)
                    }
                }
            val holder = holder(construct)
            holder.newChat()
            holder.send("Hello")
            runCurrent()
            val failed = assertNotNull(holder.state.value.thread)
            assertEquals(ChatReply.Failed("cannot save"), failed.reply)
            assertEquals(listOf(user), failed.messages)

            holder.retry()
            runCurrent()
            val answered = assertNotNull(holder.state.value.thread)
            assertEquals(listOf(user, ChatMessage(ChatRole.ASSISTANT, "Saved answer")), answered.messages)
        }

    @Test
    fun acceptedErrorBeforeStartClearsAnsweringAndKeepsThePromptForRetryAfterSettings() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val events = MutableSharedFlow<ChatEvent>()
            val settings = FakeModelSettingsStore().apply { saveProvider("https://example.test", "Model", "sample") }
            val inputs = mutableListOf<ChatInput>()
            val construct =
                object : Construct by fake {
                    override val chatEvents = events

                    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
                        inputs += input
                        if (inputs.size == 1) {
                            events.emit(ChatEvent.Error(NO_CHAT_MODEL))
                        } else {
                            events.emit(ChatEvent.Start("phone-chat"))
                            events.emit(ChatEvent.Delta("An answer"))
                            events.emit(ChatEvent.Done(stopped = false))
                        }
                        return Outcome.Ok(Unit)
                    }
                }
            val holder = holder(construct, settings)
            holder.newChat()
            holder.send("Hello")
            runCurrent()
            val state = holder.state.value
            assertFalse(state.answering)
            assertFalse(state.modelReady)
            val failed = assertNotNull(state.thread)
            assertTrue(failed.messages.isEmpty())
            assertEquals(ChatReply.Failed(NO_CHAT_MODEL), failed.reply)
            holder.retry()
            runCurrent()
            assertEquals(1, inputs.size)

            settings.saveProvider("https://example.test/v1", "Model", "sample")
            holder.refreshModel()
            runCurrent()
            holder.retry()
            runCurrent()
            assertEquals(List(2) { ChatInput("", "Hello") }, inputs)
            val answered = assertNotNull(holder.state.value.thread)
            assertEquals("phone-chat", answered.id)
            assertEquals(
                listOf(ChatMessage(ChatRole.USER, "Hello"), ChatMessage(ChatRole.ASSISTANT, "An answer")),
                answered.messages,
            )
        }

    @Test
    fun delayedMissingModelDoesNotDisableRetryAfterSettingsCommits() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val events = MutableSharedFlow<ChatEvent>()
            val finishOldSend = CompletableDeferred<Unit>()
            val settings = FakeModelSettingsStore().apply { saveProvider("not a URL", "Old model", "sample") }
            val modelsUsed = mutableListOf<String>()
            val construct =
                object : Construct by fake {
                    override val chatEvents = events

                    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
                        modelsUsed += settings.read().model
                        if (modelsUsed.size == 1) {
                            backgroundScope.launch {
                                finishOldSend.await()
                                events.emit(ChatEvent.Error(NO_CHAT_MODEL))
                            }
                        } else {
                            events.emit(ChatEvent.Start("phone-chat"))
                            events.emit(ChatEvent.Delta("An answer"))
                            events.emit(ChatEvent.Done(stopped = false))
                        }
                        return Outcome.Ok(Unit)
                    }
                }
            val holder = holder(construct, settings)
            holder.newChat()
            holder.send("Hello")
            runCurrent()
            assertTrue(holder.state.value.answering)

            settings.saveProvider("https://example.test/v1", "New model", "sample")
            holder.refreshModel()
            runCurrent()
            assertEquals("New model", holder.state.value.modelName)
            finishOldSend.complete(Unit)
            runCurrent()
            val failed = holder.state.value
            assertFalse(failed.answering)
            assertTrue(failed.modelReady)
            assertEquals(ChatReply.Failed(NO_CHAT_MODEL), failed.thread?.reply)

            holder.retry()
            runCurrent()
            assertEquals(listOf("Old model", "New model"), modelsUsed)
            val answered = assertNotNull(holder.state.value.thread)
            assertEquals(
                listOf(ChatMessage(ChatRole.USER, "Hello"), ChatMessage(ChatRole.ASSISTANT, "An answer")),
                answered.messages,
            )
        }

    @Test
    fun rejectedSendLeavesNoUnstoredMessageAndCanBeRetried() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val construct =
                object : Construct by fake {
                    private var first = true

                    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
                        if (!first) return fake.sendMessage(input)
                        first = false
                        return Outcome.Failed("key refused")
                    }
                }
            val holder = holder(construct)
            holder.newChat()
            holder.send("Hello")
            runCurrent()
            assertEquals(
                ChatReply.Failed("key refused"),
                holder.state.value.thread
                    ?.reply,
            )
            assertTrue(assertNotNull(holder.state.value.thread).messages.isEmpty())

            holder.retry()
            runCurrent()
            val thread = assertNotNull(holder.state.value.thread)
            assertEquals(listOf("Hello"), thread.messages.filter { it.role == ChatRole.USER }.map { it.text })
            assertEquals(FakeConstruct.CANNED_ANSWER.joinToString(""), thread.messages.last().text)
        }

    @Test
    fun aComputerThreadShowsItsMessagesAndCannotBeChanged() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val holder = holder(fake)
            val computer =
                holder.state.value.conversations
                    .first()
            assertEquals(ConversationOrigin.COMPUTER, computer.origin)
            holder.open(computer)
            runCurrent()
            val thread = assertNotNull(holder.state.value.thread)
            assertTrue(thread.readOnly)
            assertTrue(thread.messages.any { it.role == ChatRole.TOOL })

            holder.send("This must not be sent")
            holder.delete(computer.id)
            runCurrent()
            assertEquals(thread, holder.state.value.thread)
            assertEquals(
                computer,
                holder.state.value.conversations
                    .first(),
            )
            assertFalse(holder.state.value.answering)
        }

    @Test
    fun refreshReadsModelChangesAfterReturningFromSettings() =
        runTest {
            val settings = FakeModelSettingsStore()
            val holder = holder(FakeConstruct(backgroundScope), settings)
            holder.newChat()
            holder.send("Wait for model setup")
            assertFalse(holder.state.value.modelReady)
            assertTrue(assertNotNull(holder.state.value.thread).messages.isEmpty())

            settings.saveProvider("https://example.test/v1", "Example model", "sample")
            holder.refresh()
            runCurrent()
            assertTrue(holder.state.value.modelReady)
            assertEquals("Example model", holder.state.value.modelName)
        }

    private fun TestScope.holder(
        construct: Construct,
        settings: FakeModelSettingsStore =
            FakeModelSettingsStore().apply {
                saveProvider("https://example.test/v1", "Example model", "sample")
            },
    ): ChatHolder =
        ChatHolder(construct, settings, backgroundScope, StandardTestDispatcher(testScheduler)).also {
            runCurrent()
        }
}
