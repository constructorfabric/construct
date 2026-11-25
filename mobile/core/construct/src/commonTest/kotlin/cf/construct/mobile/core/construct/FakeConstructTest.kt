package cf.construct.mobile.core.construct

import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.valueOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class FakeConstructTest {
    private fun TestScope.fake(deltaDelayMillis: Long = 0) = FakeConstruct(this, deltaDelayMillis)

    private fun TestScope.collectEvents(construct: Construct): List<ChatEvent> {
        val seen = mutableListOf<ChatEvent>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { construct.chatEvents.collect { seen += it } }
        return seen
    }

    private suspend fun Construct.conversationIds(): List<String> =
        listConversations().valueOrNull().orEmpty().map { it.id }

    @Test
    fun streamsTheCannedAnswerAndKeepsItInTheThread() =
        runTest {
            val construct = fake()
            val seen = collectEvents(construct)
            assertEquals(Outcome.Ok(Unit), construct.sendMessage(ChatInput("c2", "And Tuesday?")))
            advanceUntilIdle()
            assertEquals(
                listOf(
                    ChatEvent.Start("c2"),
                    ChatEvent.Tool("Looked up your profile"),
                    ChatEvent.Delta("Here is "),
                    ChatEvent.Delta("a canned "),
                    ChatEvent.Delta("answer."),
                    ChatEvent.Done(stopped = false),
                ),
                seen,
            )
            assertEquals(
                listOf(
                    ChatMessage(ChatRole.USER, "Plan my week"),
                    ChatMessage(ChatRole.ASSISTANT, "Start with the release notes on Monday."),
                    ChatMessage(ChatRole.USER, "And Tuesday?"),
                    ChatMessage(ChatRole.ASSISTANT, "Here is a canned answer."),
                ),
                construct.getConversation("c2").valueOrNull(),
            )
        }

    @Test
    fun newChatsGetUniqueIdsAndListFirst() =
        runTest {
            val construct = fake()
            construct.sendMessage(ChatInput("", "Hello"))
            advanceUntilIdle()
            assertEquals(listOf("new-1", "c1", "c2"), construct.conversationIds())
            construct.deleteConversation("c1")
            construct.sendMessage(ChatInput("", "Hello again"))
            advanceUntilIdle()
            assertEquals(listOf("new-2", "new-1", "c2"), construct.conversationIds())
            assertEquals(
                "Hello again",
                construct
                    .listConversations()
                    .valueOrNull()
                    ?.first()
                    ?.title,
            )
        }

    @Test
    fun stoppingMidAnswerEndsItWithAStoppedDone() =
        runTest {
            val construct = fake(deltaDelayMillis = 1_000)
            val seen = collectEvents(construct)
            construct.sendMessage(ChatInput("c1", "Hi"))
            runCurrent()
            construct.stopAnswer()
            advanceUntilIdle()
            assertEquals(ChatEvent.Done(stopped = true), seen.last())
            assertTrue(seen.none { it is ChatEvent.Delta })
            // A stopped answer keeps the user's message and adds no reply.
            assertEquals(
                ChatMessage(ChatRole.USER, "Hi"),
                construct.getConversation("c1").valueOrNull()?.last(),
            )
        }

    @Test
    fun stoppingRightAfterSendingStillEndsWithDone() =
        runTest {
            val construct = fake(deltaDelayMillis = 1_000)
            val seen = collectEvents(construct)
            construct.sendMessage(ChatInput("c1", "Hi"))
            construct.stopAnswer()
            advanceUntilIdle()
            assertEquals(ChatEvent.Done(stopped = true), seen.last())
            assertEquals(1, seen.count { it is ChatEvent.Done })
        }

    @Test
    fun stoppingRightAfterSendingWithoutADelayStillStops() =
        runTest {
            val construct = fake()
            val seen = collectEvents(construct)
            construct.sendMessage(ChatInput("c1", "Hi"))
            construct.stopAnswer()
            advanceUntilIdle()
            assertEquals(listOf(ChatEvent.Done(stopped = true)), seen.filterIsInstance<ChatEvent.Done>())
            assertTrue(seen.none { it is ChatEvent.Delta })
        }

    @Test
    fun aNewChatAfterAStopKeepsItsOwnIdAndTime() =
        runTest {
            val construct = fake(deltaDelayMillis = 1_000)
            construct.sendMessage(ChatInput("", "First"))
            construct.stopAnswer()
            construct.sendMessage(ChatInput("", "Second"))
            advanceUntilIdle()
            val list = construct.listConversations().valueOrNull().orEmpty()
            assertEquals(listOf("new-2" to "Second", "new-1" to "First"), list.take(2).map { it.id to it.title })
        }

    @Test
    fun aSecondAnswerWhileOneRunsFails() =
        runTest {
            val construct = fake(deltaDelayMillis = 1_000)
            construct.sendMessage(ChatInput("c1", "Hi"))
            runCurrent()
            assertEquals(Outcome.Failed(FakeConstruct.ANSWER_RUNNING), construct.sendMessage(ChatInput("c1", "Again")))
            advanceUntilIdle()
        }

    @Test
    fun erasingAgainReportsThatAnErasureWasAlreadyOn() =
        runTest {
            val construct = fake()
            construct.eraseEverything()
            assertEquals(Outcome.Ok(EraseStarted(running = true)), construct.eraseEverything())
        }

    private fun Outcome<MemoryFacts>.factIds(): List<String> = valueOrNull()?.facts.orEmpty().map { it.factId }

    @Test
    fun deletingAFactIsSafeToRepeat() =
        runTest {
            val construct = fake()
            construct.deleteMemoryFact("f1")
            assertEquals(Outcome.Ok(Unit), construct.deleteMemoryFact("f1"))
            assertEquals(listOf("f2", "f3", "f4"), construct.memoryFacts.first().factIds())
        }

    @Test
    fun aCollectorThatIsAlreadyCollectingSeesADeletedFact() =
        runTest {
            val construct = fake()
            val seen = mutableListOf<Outcome<MemoryFacts>>()
            backgroundScope.launch(
                UnconfinedTestDispatcher(testScheduler),
            ) { construct.memoryFacts.collect { seen += it } }
            runCurrent()
            construct.deleteMemoryFact("f1")
            runCurrent()
            assertEquals(listOf("f2", "f3", "f4"), seen.last().factIds())
            assertEquals(2, seen.size)
        }
}
