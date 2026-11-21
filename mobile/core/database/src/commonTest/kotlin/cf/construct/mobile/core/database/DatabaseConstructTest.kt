package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DocumentProblem
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Whose
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** What only the store does, beside what [ConstructContract] asks of every implementation. */
class DatabaseConstructTest {
    private val driver = newInMemoryDriver()
    private val store = CopyStore(driver)
    private val construct = DatabaseConstruct(store, Dispatchers.Unconfined)

    @Test
    fun sendingFailsBecauseNothingAnswersYet() =
        runTest {
            assertEquals(Outcome.Failed("not available yet"), construct.sendMessage(ChatInput("c1", "Hi")))
        }

    @Test
    fun stoppingChangesNothingAndNoEventComes() =
        runTest {
            val chat = ConversationSummary("c1", "Hi", "2026-01-01T00:00:00Z")
            store.saveConversation(chat, listOf(ChatMessage(ChatRole.USER, "Hi")))
            construct.sendMessage(ChatInput("c1", "Again"))
            construct.stopAnswer()
            assertEquals(emptyList(), construct.chatEvents.toList())
            assertEquals(Outcome.Ok(listOf(chat)), construct.listConversations())
            assertEquals(Outcome.Ok(listOf(ChatMessage(ChatRole.USER, "Hi"))), construct.getConversation("c1"))
        }

    @Test
    fun anEmptyCopyAnswersWithEmptyReads() =
        runTest {
            assertEquals(Outcome.Ok(emptyList<ConversationSummary>()), construct.listConversations())
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), null)), construct.getMemoryFacts())
            assertEquals(Outcome.Ok(DocumentState(null, null)), construct.getDashboardDocument())
        }

    @Test
    fun numbersThatWereNeverStoredFail() =
        runTest {
            assertEquals(Outcome.Failed("no numbers for these days"), construct.getDashboardNumbers(contractInput))
        }

    @Test
    fun numbersAreKeptForExactlyTheInputTheyWereCountedFor() =
        runTest {
            store.saveNumbers(contractInput, contractNumbers)
            val otherSessions = (contractInput as DashboardInput.ForSessions).copy(sessions = Whose.ALL)
            val otherDays = DashboardInput.ForSessions(contractInput.days.take(1), Whose.OWN)
            assertEquals(Outcome.Ok(contractNumbers), construct.getDashboardNumbers(contractInput))
            assertEquals(Outcome.Failed("no numbers for these days"), construct.getDashboardNumbers(otherSessions))
            assertEquals(Outcome.Failed("no numbers for these days"), construct.getDashboardNumbers(otherDays))
        }

    @Test
    fun aRowThatCannotBeReadFailsTheReadAndNotTheCaller() =
        runTest {
            store.saveFacts(MemoryFacts(emptyList(), 2L))
            driver.execute(
                null,
                "INSERT INTO fact VALUES ('f9', 'ROLE', 'title', 'not json', '2026-01-01T08:00:00Z', NULL)",
                0,
            )
            driver.execute(
                null,
                "INSERT INTO fact VALUES ('f8', 'GONE', 'title', '\"x\"', '2026-01-01T08:00:00Z', NULL)",
                0,
            )
            assertEquals(Outcome.Failed(DatabaseConstruct.UNREADABLE), construct.getMemoryFacts())
            assertEquals(Outcome.Ok(2L), construct.getMemoryVersion())
            construct.deleteMemoryFact("f9")
            assertEquals(Outcome.Failed(DatabaseConstruct.UNREADABLE), construct.getMemoryFacts())
            construct.deleteMemoryFact("f8")
            assertEquals(Outcome.Ok(MemoryFacts(emptyList(), 2L)), construct.getMemoryFacts())
        }

    @Test
    fun aKeyTheTypeNoLongerHasIsIgnoredWhenAStoredRowIsRead() =
        runTest {
            val row = "'{\"title\":\"Week\",\"period\":\"TODAY\",\"sessions\":\"ALL\",\"widgets\":[],\"removed\":1}'"
            driver.execute(null, "INSERT INTO document VALUES (1, $row, NULL)", 0)
            val document = DashboardDocument("Week", Period.TODAY, Whose.ALL, emptyList())
            assertEquals(Outcome.Ok(DocumentState(document, null)), construct.getDashboardDocument())
        }

    @Test
    fun savingFactsReplacesTheFactsThatWereThere() {
        val first = MemoryFact("f1", EntityKind.ROLE, "title", JsonPrimitive("A"), "2026-01-01T08:00:00Z")
        val second = MemoryFact("f2", EntityKind.ROLE, "title", JsonPrimitive("B"), "2026-01-01T08:01:00Z")
        val third = MemoryFact("f3", EntityKind.ROLE, "title", JsonPrimitive("C"), "2026-01-01T08:02:00Z")
        store.saveFacts(MemoryFacts(listOf(first, second), 1L))
        store.saveFacts(MemoryFacts(listOf(third), 2L))
        assertEquals(MemoryFacts(listOf(third), 2L), store.facts())
    }

    @Test
    fun erasingEmptiesTheDashboardAndTheNumbersToo() =
        runTest {
            store.saveDocument(DocumentState(FakeConstruct.seededDocument, null))
            store.saveNumbers(contractInput, contractNumbers)
            store.markSynced("2026-01-02T10:00:00Z")
            construct.eraseEverything()
            assertEquals(Outcome.Ok(DocumentState(null, null)), construct.getDashboardDocument())
            assertEquals(Outcome.Failed("no numbers for these days"), construct.getDashboardNumbers(contractInput))
            assertNull(store.lastSync())
        }

    @Test
    fun savingAConversationAgainReplacesItsMessages() {
        val chat = ConversationSummary("c1", "Hi", "2026-01-01T00:00:00Z")
        store.saveConversation(chat, listOf(ChatMessage(ChatRole.USER, "Hi"), ChatMessage(ChatRole.TOOL, "Looked")))
        store.saveConversation(chat, listOf(ChatMessage(ChatRole.USER, "Hi")))
        assertEquals(listOf(ChatMessage(ChatRole.USER, "Hi")), store.messages("c1"))
    }

    @Test
    fun aConversationWithoutMessagesIsStillThere() {
        store.saveConversation(ConversationSummary("c1", "Empty", "2026-01-01T00:00:00Z"), emptyList())
        assertEquals(emptyList(), store.messages("c1"))
        assertNull(store.messages("c2"))
    }

    @Test
    fun aFactWithFieldsOfItsOwnKeepsThem() {
        val value = buildJsonObject { put("city", "Paris") }
        val fact = MemoryFact("f1", EntityKind.IDENTITY, "home", value, "2026-01-01T08:00:00Z", "editor")
        store.saveFacts(MemoryFacts(listOf(fact), 7L))
        assertEquals(MemoryFacts(listOf(fact), 7L), store.facts())
    }

    @Test
    fun theProblemOfTheDocumentIsKeptWithTheLastGoodOne() {
        val document = DashboardDocument("Week", Period.TODAY, Whose.ALL, emptyList())
        val problem = DocumentProblem(DocumentProblem.Kind.LINE, "3:7", "unexpected character")
        store.saveDocument(DocumentState(document, problem))
        assertEquals(DocumentState(document, problem), store.document())
    }

    @Test
    fun theLastSyncIsKeptAndEmptiedWithEverythingElse() {
        assertNull(store.lastSync())
        store.markSynced("2026-01-02T10:00:00Z")
        store.markSynced("2026-01-03T10:00:00Z")
        assertEquals("2026-01-03T10:00:00Z", store.lastSync())
        store.deleteEverything()
        assertNull(store.lastSync())
    }

    @Test
    fun savingFactsKeepsTheTimeOfTheLastSync() {
        store.markSynced("2026-01-02T10:00:00Z")
        store.saveFacts(MemoryFacts(emptyList(), 1L))
        assertEquals("2026-01-02T10:00:00Z", store.lastSync())
    }
}
