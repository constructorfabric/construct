package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.construct.NO_SUCH_CONVERSATION
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.valueOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

val contractInput: DashboardInput =
    DashboardInput.ForSessions(
        listOf(
            DayBounds("Mon", "2026-01-05T00:00Z", "2026-01-06T00:00Z"),
            DayBounds("Tue", "2026-01-06T00:00Z", "2026-01-07T00:00Z"),
        ),
        Whose.OWN,
    )

/** The numbers every implementation gives for [contractInput]: 60 and 120 minutes over two days. */
val contractNumbers =
    DashboardNumbers(
        totalMinutes = 180,
        sessionCount = 4,
        days = listOf(DayMinutes("Mon", 60), DayMinutes("Tue", 120)),
        groups = listOf(GroupMinutes("alpha", 126, 0.7, 1), GroupMinutes("beta", 54, 0.3, 2)),
    )

/** Collects the facts of [construct] from now on, as a screen that stays open does, and returns what it has seen. */
@OptIn(ExperimentalCoroutinesApi::class)
fun TestScope.collectFacts(construct: Construct): List<Outcome<MemoryFacts>> {
    val seen = mutableListOf<Outcome<MemoryFacts>>()
    backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { construct.memoryFacts.collect { seen += it } }
    return seen
}

/**
 * What every [Construct] must answer for the reads and the deletes, whatever keeps its data. Each
 * implementation holds the data [FakeConstruct] is seeded with, and [contractNumbers] for [contractInput].
 */
@OptIn(ExperimentalCoroutinesApi::class)
abstract class ConstructContract {
    abstract suspend fun TestScope.construct(): Construct

    private suspend fun Construct.conversationIds(): List<String> =
        listConversations().valueOrNull().orEmpty().map { it.id }

    private fun Outcome<MemoryFacts>.factIds(): List<String> = valueOrNull()?.facts.orEmpty().map { it.factId }

    private suspend fun Construct.factIds(): List<String> = memoryFacts.first().factIds()

    @Test
    fun listsTheConversationsNewestFirst() =
        runTest {
            val newestFirst =
                FakeConstruct.seededConversations.keys
                    .sortedByDescending { it.time }
                    .map { it.id }
            assertEquals(newestFirst, construct().conversationIds())
        }

    @Test
    fun givesTheMessagesOfAConversationInOrder() =
        runTest {
            for ((summary, messages) in FakeConstruct.seededConversations) {
                assertEquals(messages, construct().getConversation(summary.id).valueOrNull())
            }
        }

    @Test
    fun anUnknownConversationFails() =
        runTest {
            assertEquals(Outcome.Failed(NO_SUCH_CONVERSATION), construct().getConversation("nope"))
        }

    @Test
    fun deletesOneConversationAndThenAll() =
        runTest {
            val construct = construct()
            construct.deleteConversation("c1")
            assertEquals(listOf("c2"), construct.conversationIds())
            assertEquals(Outcome.Failed(NO_SUCH_CONVERSATION), construct.getConversation("c1"))
            construct.deleteConversation(DELETE_ALL_CONVERSATIONS)
            assertTrue(construct.conversationIds().isEmpty())
        }

    @Test
    fun givesTheFactsInOrderWithTheirToolAndTheVersion() =
        runTest {
            val facts = construct().memoryFacts.first().valueOrNull()
            assertEquals(FakeConstruct.seededFacts, facts?.facts)
            assertEquals(FakeConstruct.FACTS_VERSION, facts?.version)
            assertNull(facts?.facts?.first()?.tool)
        }

    @Test
    fun givesTheVersionAlone() =
        runTest {
            assertEquals(Outcome.Ok(FakeConstruct.FACTS_VERSION), construct().getMemoryVersion())
        }

    @Test
    fun deletingAFactIsSafeToRepeat() =
        runTest {
            val construct = construct()
            construct.deleteMemoryFact("f1")
            assertEquals(Outcome.Ok(Unit), construct.deleteMemoryFact("f1"))
            assertEquals(listOf("f2", "f3", "f4"), construct.factIds())
        }

    @Test
    fun aCollectorThatIsAlreadyCollectingSeesADeletedFactAndTheErasure() =
        runTest {
            val construct = construct()
            val seen = collectFacts(construct)
            runCurrent()
            assertEquals(listOf(FakeConstruct.seededFacts.map { it.factId }), seen.map { it.factIds() })

            construct.deleteMemoryFact("f1")
            runCurrent()
            assertEquals(listOf("f2", "f3", "f4"), seen.last().factIds())

            construct.eraseEverything()
            runCurrent()
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), null)), seen.last())
            assertEquals(3, seen.size)
        }

    @Test
    fun aCollectorThatStartsLaterGetsTheFactsAsTheyAreThen() =
        runTest {
            val construct = construct()
            construct.deleteMemoryFact("f1")
            assertEquals(listOf("f2", "f3", "f4"), construct.memoryFacts.first().factIds())
        }

    @Test
    fun erasingEmptiesTheMemoryAndTheChats() =
        runTest {
            val construct = construct()
            assertEquals(Outcome.Ok(EraseStarted(running = false)), construct.eraseEverything())
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), null)), construct.memoryFacts.first())
            assertEquals(Outcome.Ok(null), construct.getMemoryVersion())
            assertEquals(Outcome.Ok(false), construct.getErasing())
            assertTrue(construct.conversationIds().isEmpty())
        }

    @Test
    fun givesTheDashboardNumbersForTheDaysAsked() =
        runTest {
            assertEquals(Outcome.Ok(contractNumbers), construct().getDashboardNumbers(contractInput))
        }

    @Test
    fun givesTheDocumentWithoutAProblem() =
        runTest {
            val state = construct().getDashboardDocument().valueOrNull()
            assertEquals(FakeConstruct.seededDocument, state?.document)
            assertNull(state?.problem)
        }
}
