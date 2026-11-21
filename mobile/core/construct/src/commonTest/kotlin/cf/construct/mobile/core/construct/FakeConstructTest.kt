package cf.construct.mobile.core.construct

import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.valueOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class FakeConstructTest {
    private fun TestScope.fake(deltaDelayMillis: Long = 0) = FakeConstruct(this, deltaDelayMillis)

    private fun TestScope.collectEvents(construct: Construct): List<ChatEvent> {
        val seen = mutableListOf<ChatEvent>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { construct.chatEvents.collect { seen += it } }
        return seen
    }

    @Test
    fun listsTheSeededConversationsNewestFirst() =
        runTest {
            val list = fake().listConversations().valueOrNull().orEmpty()
            assertEquals(listOf("c1", "c2"), list.map { it.id })
        }

    @Test
    fun unknownConversationFails() =
        runTest {
            assertEquals(Outcome.Failed(FakeConstruct.NO_SUCH_CONVERSATION), fake().getConversation("nope"))
        }

    @Test
    fun deleteOneAndDeleteAll() =
        runTest {
            val construct = fake()
            construct.deleteConversation("c1")
            assertEquals(
                listOf("c2"),
                construct
                    .listConversations()
                    .valueOrNull()
                    .orEmpty()
                    .map { it.id },
            )
            construct.deleteConversation(DELETE_ALL_CONVERSATIONS)
            assertTrue(
                construct
                    .listConversations()
                    .valueOrNull()
                    .orEmpty()
                    .isEmpty(),
            )
        }

    @Test
    fun streamsTheCannedAnswerAndKeepsItInTheThread() =
        runTest {
            val construct = fake()
            val seen = collectEvents(construct)
            assertIs<Outcome.Ok<Unit>>(construct.sendMessage(ChatInput("c2", "And Tuesday?")))
            advanceUntilIdle()
            assertEquals(
                listOf(ChatEvent.Start("c2"), ChatEvent.Tool(FakeConstruct.CANNED_TOOL_LINE)) +
                    FakeConstruct.CANNED_ANSWER.map { ChatEvent.Delta(it) } + ChatEvent.Done(stopped = false),
                seen,
            )
            val thread = construct.getConversation("c2").valueOrNull().orEmpty()
            assertEquals(FakeConstruct.CANNED_ANSWER.joinToString(""), thread.last().text)
        }

    @Test
    fun anEmptyConversationStartsANewOne() =
        runTest {
            val construct = fake()
            val seen = collectEvents(construct)
            construct.sendMessage(ChatInput("", "Hello"))
            advanceUntilIdle()
            val started = assertIs<ChatEvent.Start>(seen.first())
            assertEquals(
                "Hello",
                construct
                    .listConversations()
                    .valueOrNull()
                    .orEmpty()
                    .first {
                        it.id ==
                            started.conversation
                    }.title,
            )
        }

    @Test
    fun stoppingEndsTheAnswerWithAStoppedDone() =
        runTest {
            val construct = fake(deltaDelayMillis = 1_000)
            val seen = collectEvents(construct)
            construct.sendMessage(ChatInput("c1", "Hi"))
            runCurrent()
            construct.stopAnswer()
            advanceUntilIdle()
            assertEquals(ChatEvent.Done(stopped = true), seen.last())
            assertTrue(seen.none { it is ChatEvent.Delta })
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
    fun deletingAFactIsSafeToRepeat() =
        runTest {
            val construct = fake()
            construct.deleteMemoryFact("f1")
            assertEquals(Outcome.Ok(Unit), construct.deleteMemoryFact("f1"))
            val facts = construct.getMemoryFacts().valueOrNull()
            assertEquals(listOf("f2", "f3", "f4"), facts?.facts?.map { it.factId })
        }

    @Test
    fun aFactMayHaveNoTool() =
        runTest {
            val facts =
                fake()
                    .getMemoryFacts()
                    .valueOrNull()
                    ?.facts
                    .orEmpty()
            assertNull(facts.first().tool)
            assertEquals("editor", facts.first { it.factId == "f3" }.tool)
        }

    @Test
    fun erasingEmptiesTheMemoryAndTheVersion() =
        runTest {
            val construct = fake()
            assertEquals(Outcome.Ok(EraseStarted(running = false)), construct.eraseEverything())
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), null)), construct.getMemoryFacts())
            assertEquals(Outcome.Ok(false), construct.getErasing())
            assertTrue(
                construct
                    .listConversations()
                    .valueOrNull()
                    .orEmpty()
                    .isEmpty(),
            )
        }

    @Test
    fun dashboardNumbersFollowTheDaysAsked() =
        runTest {
            val days =
                listOf(
                    DayBounds("Mon", "2026-01-05T00:00Z", "2026-01-06T00:00Z"),
                    DayBounds("Tue", "2026-01-06T00:00Z", "2026-01-07T00:00Z"),
                )
            val numbers = fake().getDashboardNumbers(DashboardInput.ForSessions(days, Whose.OWN)).valueOrNull()
            assertEquals(listOf("Mon", "Tue"), numbers?.days?.map { it.label })
            assertEquals(numbers?.days?.sumOf { it.minutes }, numbers?.totalMinutes)
        }

    @Test
    fun theSeededDocumentHasWidgets() =
        runTest {
            val state = fake().getDashboardDocument().valueOrNull()
            assertEquals(3, state?.document?.widgets?.size)
            assertNull(state?.problem)
        }
}
