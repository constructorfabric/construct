package cf.construct.mobile.core.construct

import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.GroupMinutes
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

    private suspend fun Construct.conversationIds(): List<String> =
        listConversations().valueOrNull().orEmpty().map { it.id }

    @Test
    fun listsTheSeededConversationsNewestFirst() =
        runTest {
            assertEquals(listOf("c1", "c2"), fake().conversationIds())
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
            assertEquals(listOf("c2"), construct.conversationIds())
            construct.deleteConversation(DELETE_ALL_CONVERSATIONS)
            assertTrue(construct.conversationIds().isEmpty())
        }

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
    fun theVersionIsTheProfilesUntilEverythingIsErased() =
        runTest {
            val construct = fake()
            assertEquals(Outcome.Ok(3L), construct.getMemoryVersion())
            construct.eraseEverything()
            assertEquals(Outcome.Ok(null), construct.getMemoryVersion())
        }

    @Test
    fun erasingEmptiesTheMemoryAndTheChats() =
        runTest {
            val construct = fake()
            assertEquals(Outcome.Ok(EraseStarted(running = false)), construct.eraseEverything())
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), null)), construct.getMemoryFacts())
            assertEquals(Outcome.Ok(false), construct.getErasing())
            assertTrue(construct.conversationIds().isEmpty())
        }

    @Test
    fun erasingAgainReportsThatAnErasureWasAlreadyOn() =
        runTest {
            val construct = fake()
            construct.eraseEverything()
            assertEquals(Outcome.Ok(EraseStarted(running = true)), construct.eraseEverything())
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
            assertEquals(listOf(DayMinutes("Mon", 60), DayMinutes("Tue", 120)), numbers?.days)
            assertEquals(180, numbers?.totalMinutes)
            assertEquals(4, numbers?.sessionCount)
            assertEquals(
                listOf(GroupMinutes("alpha", 126, 0.7, 1), GroupMinutes("beta", 54, 0.3, 2)),
                numbers?.groups,
            )
        }

    @Test
    fun theSeededDocumentHasItsWidgets() =
        runTest {
            val state = fake().getDashboardDocument().valueOrNull()
            assertEquals(listOf("total", "days", "by-project"), state?.document?.widgets?.map { it.id })
            assertNull(state?.problem)
        }
}
