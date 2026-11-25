package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Whose
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** What the copy does for a sync: the computer it came from, the reset, and one whole update. */
@OptIn(ExperimentalCoroutinesApi::class)
class CopySyncTest {
    private val store = CopyStore(newInMemoryDriver())
    private val construct = DatabaseConstruct(store, Dispatchers.Unconfined)

    private val fact = MemoryFact("f1", EntityKind.SKILL, "skills", JsonPrimitive("Rust"), "2026-11-01T10:00:00Z")
    private val other = MemoryFact("f2", EntityKind.ROLE, "title", JsonPrimitive("Engineer"), "2026-11-02T10:00:00Z")
    private val input = DashboardInput.ForWidget(listOf(DayBounds("2026-11-01", "a", "b")), "total")
    private val numbers = DashboardNumbers(60, 1, emptyList(), emptyList(), hiddenProjectCount = 2)
    private val document = DashboardDocument("Week", Period.THIS_WEEK, Whose.OWN, emptyList())

    private fun chat(
        id: String,
        vararg lines: String,
        from: Int = 0,
        time: String? = "2026-11-01T10:00:00Z",
    ) = ChatUpdate(id, "Chat $id", time, from, lines.map { ChatMessage(ChatRole.USER, it) })

    private fun update(
        marker: String? = "m1",
        facts: FactsUpdate? = FactsUpdate("tf1", listOf(fact)),
        chats: ChatsUpdate? = ChatsUpdate("tc1", listOf(chat("c1", "Hi", "There"))),
        dashboard: DashboardUpdate? = DashboardUpdate("td1", "Europe/Berlin", document, listOf(input to numbers)),
        at: String = "2026-11-03T10:00:00Z",
        computer: String = "A",
    ) = CopyUpdate(computer, at, marker, facts, chats, dashboard)

    private fun startPhoneChat() =
        store.appendPhoneRecord(
            PhoneRecord("phone", "2026-11-04T10:00:00Z", ChatMessage(ChatRole.USER, "Mine")),
            0,
            "phone record",
        )

    @Test
    fun aCopyWithNoComputerTakesTheFirstOneAndKeepsWhatItHolds() {
        store.seedWithSampleData()
        store.adoptComputer("A")
        assertEquals("A", store.syncRecord().computer)
        assertFalse(store.syncRecord().firstCopyDone)
        assertEquals(FakeConstruct.seededFacts, store.facts().facts)
        assertTrue(store.conversations().isNotEmpty())
    }

    @Test
    fun theSameComputerAgainKeepsTheCopyAndTheTokens() {
        store.adoptComputer("A")
        store.applySync(update())
        store.adoptComputer("A")
        val record = store.syncRecord()
        assertEquals("A", record.computer)
        assertTrue(record.firstCopyDone)
        assertEquals("tf1", record.factsToken)
        assertEquals(listOf(fact), store.facts().facts)
        assertEquals(numbers, store.numbers(input))
    }

    @Test
    fun theSameComputerAgainForgetsTheResetMarkerSoANewOneIsNotTakenForAnErase() {
        store.adoptComputer("A")
        store.applySync(update(marker = "m1"))
        startPhoneChat()

        store.adoptComputer("A")

        assertNull(store.syncRecord().resetMarker)
        assertEquals(listOf("phone", "c1").sorted(), store.conversations().map { it.id }.sorted())
    }

    @Test
    fun anotherComputerEmptiesWhatCameFromTheOldOneAndKeepsWhatThePhoneStarted() {
        store.adoptComputer("A")
        store.applySync(update())
        startPhoneChat()

        store.adoptComputer("B")

        assertEquals(SyncRecord(computer = "B"), store.syncRecord())
        assertEquals(MemoryFacts(emptyList(), null), store.facts())
        assertEquals(DocumentState(null, null), store.document())
        assertNull(store.numbers(input))
        assertEquals(listOf("phone"), store.conversations().map { it.id })
        assertEquals(listOf("phone record"), store.chatRecords("phone"))
    }

    @Test
    fun aResetEmptiesTheCopyAndThePhoneChatsAndKeepsTheComputerAndTheFirstCopy() {
        store.adoptComputer("A")
        store.applySync(update())
        startPhoneChat()

        store.reset("m2")

        val record = store.syncRecord()
        assertEquals("A", record.computer)
        assertEquals("m2", record.resetMarker)
        assertTrue(record.firstCopyDone)
        assertNull(record.factsToken)
        assertNull(record.chatsToken)
        assertNull(record.dashboardToken)
        assertEquals(emptyList(), store.conversations())
        assertEquals(emptyList(), store.chatRecords("phone"))
        assertEquals(MemoryFacts(emptyList(), null), store.facts())
        assertNull(store.numbers(input))

        startPhoneChat()
        assertEquals(listOf("phone"), store.conversations().map { it.id })
    }

    @Test
    fun aSyncIsAppliedWholeAndRecordsWhatItBroughtUp() {
        store.adoptComputer("A")
        store.applySync(update())

        assertEquals(MemoryFacts(listOf(fact), 1L), store.facts())
        assertEquals(
            listOf(ChatMessage(ChatRole.USER, "Hi"), ChatMessage(ChatRole.USER, "There")),
            store.messages("c1"),
        )
        assertEquals(ConversationOrigin.COMPUTER, store.conversations().single().origin)
        assertEquals(DocumentState(document, null), store.document())
        assertEquals(numbers, store.numbers(input))
        assertEquals(
            SyncRecord("A", "m1", "tf1", "tc1", "td1", "Europe/Berlin", true, "2026-11-03T10:00:00Z"),
            store.syncRecord(),
        )
    }

    @Test
    fun aPartThatDidNotChangeIsLeftAlone() {
        store.adoptComputer("A")
        store.applySync(update())
        store.applySync(update(facts = null, chats = null, dashboard = null, at = "2026-11-03T10:01:00Z"))

        val record = store.syncRecord()
        assertEquals("2026-11-03T10:01:00Z", record.syncedAt)
        assertEquals("tf1", record.factsToken)
        assertEquals(listOf(fact), store.facts().facts)
        assertEquals(2, store.messages("c1")?.size)
        assertEquals(numbers, store.numbers(input))
    }

    @Test
    fun whatTheComputerNoLongerHasIsRemoved() {
        store.adoptComputer("A")
        store.applySync(update(facts = FactsUpdate("tf1", listOf(fact, other))))
        store.applySync(
            update(
                facts = FactsUpdate("tf2", listOf(other)),
                chats = ChatsUpdate("tc2", listOf(chat("c2", "New"))),
                dashboard = DashboardUpdate("td2", "Europe/Berlin", null, emptyList()),
            ),
        )

        assertEquals(listOf(other), store.facts().facts)
        assertEquals(listOf("c2"), store.conversations().map { it.id })
        assertNull(store.messages("c1"))
        assertEquals(DocumentState(null, null), store.document())
        assertNull(store.numbers(input))
    }

    @Test
    fun aChatThatGrewContinuesWhatTheCopyHoldsAndKeepsItsTitleWhenNothingIsNew() {
        store.adoptComputer("A")
        store.applySync(update())
        store.applySync(
            update(chats = ChatsUpdate("tc2", listOf(chat("c1", "Third", from = 2, time = "2026-11-05T10:00:00Z")))),
        )
        assertEquals(
            listOf("Hi", "There", "Third"),
            store.messages("c1")?.map { it.text },
        )
        assertEquals("2026-11-05T10:00:00Z", store.conversations().single().time)

        store.applySync(update(chats = ChatsUpdate("tc3", listOf(chat("c1", from = 3, time = null)))))
        assertEquals("2026-11-05T10:00:00Z", store.conversations().single().time)
        assertEquals(3, store.computerChats().single().items)
    }

    @Test
    fun aChatThatContinuesWhatTheCopyDoesNotHoldFailsTheWholeUpdate() {
        store.adoptComputer("A")
        store.applySync(update())

        assertFailsWith<CopyChanged> {
            store.applySync(
                update(
                    facts = FactsUpdate("tf9", listOf(other)),
                    chats = ChatsUpdate("tc9", listOf(chat("c1", "Late", from = 5))),
                ),
            )
        }

        // Nothing of it stays: the facts and the tokens are as they were.
        assertEquals(listOf(fact), store.facts().facts)
        assertEquals("tf1", store.syncRecord().factsToken)
        assertEquals("tc1", store.syncRecord().chatsToken)
    }

    @Test
    fun anUpdateFromAnotherComputerThanTheCopysIsRefused() {
        store.adoptComputer("B")
        assertFailsWith<CopyChanged> { store.applySync(update(computer = "A")) }
        assertEquals(SyncRecord(computer = "B"), store.syncRecord())
        assertEquals(emptyList(), store.conversations())
    }

    @Test
    fun aChatOfTheComputerNeverTakesThePlaceOfAChatOfThePhone() {
        startPhoneChat()
        store.adoptComputer("A")
        store.applySync(update(chats = ChatsUpdate("tc1", listOf(chat("phone", "Theirs")))))

        assertEquals(listOf(ChatMessage(ChatRole.USER, "Mine")), store.messages("phone"))
        assertEquals(ConversationOrigin.PHONE, store.conversations().single().origin)
    }

    @Test
    fun theVersionOfTheFactsMovesOnlyWhenTheFactsDo() {
        store.adoptComputer("A")
        store.applySync(update())
        store.applySync(update(facts = FactsUpdate("tf2", listOf(fact))))
        assertEquals(1L, store.memoryVersion())
        assertEquals("tf2", store.syncRecord().factsToken)

        store.applySync(update(facts = FactsUpdate("tf3", listOf(fact, other))))
        assertEquals(2L, store.memoryVersion())

        store.applySync(update(facts = FactsUpdate("tf4", emptyList())))
        assertNull(store.memoryVersion())
    }

    @Test
    fun aSyncThatChangesNothingGivesNothingToAPageThatIsOpen() =
        runTest {
            store.adoptComputer("A")
            store.applySync(update())
            val facts = mutableListOf<Outcome<MemoryFacts>>()
            val chats = mutableListOf<Unit>()
            val dashboard = mutableListOf<Unit>()
            val scope = UnconfinedTestDispatcher(testScheduler)
            backgroundScope.launch(scope) { construct.memoryFacts.collect { facts += it } }
            backgroundScope.launch(scope) { construct.conversationChanges.collect { chats += it } }
            backgroundScope.launch(scope) { construct.dashboardChanges.collect { dashboard += it } }
            runCurrent()
            assertEquals(1, facts.size)

            store.applySync(update(facts = null, chats = null, dashboard = null, at = "2026-11-03T10:01:00Z"))
            runCurrent()
            assertEquals(1, facts.size)
            assertEquals(1, chats.size)
            assertEquals(1, dashboard.size)

            store.applySync(update(facts = FactsUpdate("tf2", listOf(fact, other))))
            runCurrent()
            assertEquals(listOf(fact, other), (facts.last() as Outcome.Ok).value.facts)
            assertTrue(chats.size >= 2)
            assertTrue(dashboard.size >= 2)
            assertEquals(emptyList(), construct.chatEvents.toList())
        }
}
