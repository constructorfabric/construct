package cf.construct.mobile.core.sync

import app.cash.sqldelight.db.QueryResult
import app.cash.sqldelight.db.SqlCursor
import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.db.SqlPreparedStatement
import app.cash.sqldelight.driver.jdbc.sqlite.JdbcSqliteDriver
import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.database.ChatsUpdate
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.CopyUpdate
import cf.construct.mobile.core.database.sql.ConstructDatabase
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.GroupBy
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SyncEngineTest {
    private val driver = JdbcSqliteDriver(JdbcSqliteDriver.IN_MEMORY).also { ConstructDatabase.Schema.create(it) }
    private val failing = FailingDriver(driver)
    private val copy = CopyStore(failing)
    private val computer = FakeComputer()
    private val engine = SyncEngine(computer, copy, now = { "2026-11-10T12:00:00Z" })

    /** A chat the user started on the phone, as the phone's own chat writes it. */
    private fun phoneChat() {
        driver.execute(
            null,
            "INSERT OR IGNORE INTO conversation VALUES ('phone', 'Mine', '2026-11-04T10:00:00Z', 'phone')",
            0,
        )
        driver.execute(null, "INSERT OR IGNORE INTO message VALUES ('phone', 0, 'USER', 'Mine')", 0)
        driver.execute(null, "INSERT OR IGNORE INTO chat_record VALUES ('phone', 0, 'record')", 0)
    }

    private fun serveSomething() {
        computer.facts = listOf(fact("f1"), fact("f2"), fact("f3"))
        computer.chats["c1"] =
            FakeComputer.FakeChat(
                "First",
                "2026-11-01T10:00:00Z",
                listOf(message("user", "Hi"), message("assistant", "Hello")),
            )
        computer.chats["c2"] =
            FakeComputer.FakeChat(
                "Second",
                "2026-11-02T10:00:00Z",
                listOf(message("user", "More"), toolLine("Looked it up"), message("assistant", "Done")),
            )
        computer.numbers =
            listOf(
                numbersItem("total", "2026-11-01", 60),
                numbersItem("total", "2026-11-02", 90),
                numbersItem("per-day", "2026-11-02", 30),
            )
    }

    private fun input(
        widget: String,
        day: String,
    ) = DashboardInput.ForWidget(listOf(DayBounds(day, "${day}T00:00:00Z", "${day}T23:59:00Z")), widget)

    @Test
    fun theFirstCopyGivesWhatTheComputerServesAndSaysHowFarEachStepIs() =
        runTest {
            serveSomething()
            val seen = mutableListOf<StepProgress>()
            assertEquals(SyncResult.Done, engine.sync { seen += it })

            assertEquals(listOf("f1", "f2", "f3"), copy.facts().facts.map { it.factId })
            assertEquals(
                EntityKind.SKILL,
                copy
                    .facts()
                    .facts
                    .first()
                    .entityKind,
            )
            assertEquals(listOf("Hi", "Hello"), copy.messages("c1")?.map { it.text })
            assertEquals(
                listOf(ChatRole.USER, ChatRole.TOOL, ChatRole.ASSISTANT),
                copy.messages("c2")?.map { it.role },
            )
            assertEquals(60, copy.numbers(input("total", "2026-11-01"))?.totalMinutes)
            assertEquals(30, copy.numbers(input("per-day", "2026-11-02"))?.totalMinutes)
            assertTrue(copy.conversations().all { it.origin == ConversationOrigin.COMPUTER })
            val record = copy.syncRecord()
            assertEquals("computer-a", record.computer)
            assertEquals("marker-1", record.resetMarker)
            assertEquals("Europe/Berlin", record.zone)
            assertTrue(record.firstCopyDone)

            // The steps come in order, the counts rise, and each step ends done with its total.
            assertEquals(listOf(SyncStep.MEMORY, SyncStep.CHATS, SyncStep.DASHBOARD), seen.map { it.step }.distinct())
            assertEquals(StepProgress(SyncStep.MEMORY, 3, 3, done = true), seen.last { it.step == SyncStep.MEMORY })
            assertEquals(StepProgress(SyncStep.CHATS, 2, 2, done = true), seen.last { it.step == SyncStep.CHATS })
            assertEquals(
                StepProgress(SyncStep.DASHBOARD, 3, 3, done = true),
                seen.last { it.step == SyncStep.DASHBOARD },
            )
            val facts = seen.filter { it.step == SyncStep.MEMORY }.map { it.copied }
            assertEquals(facts.sorted(), facts)
            assertTrue(facts.any { it in 1..2 }, "a step reports its count as pages come")
        }

    @Test
    fun aCheckThatFindsNothingNewCopiesNothing() =
        runTest {
            serveSomething()
            engine.sync()
            computer.requests.clear()

            assertEquals(SyncResult.Done, SyncEngine(computer, copy, now = { "2026-11-10T12:01:00Z" }).sync())

            assertEquals(listOf("sync-check"), computer.commands())
            assertEquals("2026-11-10T12:01:00Z", copy.syncRecord().syncedAt)
            assertEquals(3, copy.facts().facts.size)
        }

    @Test
    fun aNewFactAndAMessageAreCopiedAndOnlyTheChatThatGrewIsRead() =
        runTest {
            serveSomething()
            engine.sync()
            computer.requests.clear()
            computer.facts += fact("f4")
            computer.chats["c1"]!!.items += message("user", "One more", "2026-11-09T09:00:00Z")
            computer.chats["c3"] =
                FakeComputer.FakeChat("Third", "2026-11-09T10:00:00Z", listOf(message("user", "New chat")))

            assertEquals(SyncResult.Done, engine.sync())

            assertEquals(listOf("f1", "f2", "f3", "f4"), copy.facts().facts.map { it.factId })
            assertEquals(listOf("Hi", "Hello", "One more"), copy.messages("c1")?.map { it.text })
            assertEquals(listOf("New chat"), copy.messages("c3")?.map { it.text })
            assertEquals(2L, copy.facts().version)
            val chatReads = computer.requests.filter { it["cmd"]!!.toString().contains("sync-chat\"") }
            // c1 is read from its third line, c3 from its first, and c2, which did not grow, not at all.
            assertEquals(setOf("c1", "c3"), chatReads.map { (it["id"] as JsonPrimitive).content }.toSet())
            assertEquals(
                2,
                (
                    chatReads.first {
                        (it["id"] as JsonPrimitive).content == "c1"
                    }["after"] as JsonPrimitive
                ).content.toInt(),
            )
            assertEquals("2026-11-09T09:00:00Z", copy.conversations().first { it.id == "c1" }.time)
            assertFalse("sync-dashboard" in computer.commands())
        }

    @Test
    fun aFactAndAChatDeletedOnTheComputerAreDeletedAtTheNextSync() =
        runTest {
            serveSomething()
            engine.sync()
            computer.facts = computer.facts.filter { it["id"] != JsonPrimitive("f2") }
            computer.chats.remove("c2")

            engine.sync()

            assertEquals(listOf("f1", "f3"), copy.facts().facts.map { it.factId })
            assertEquals(listOf("c1"), copy.conversations().map { it.id })
            assertNull(copy.messages("c2"))
        }

    @Test
    fun numbersThatTheComputerNoLongerHasAreRemoved() =
        runTest {
            serveSomething()
            engine.sync()
            computer.numbers = computer.numbers.take(1)
            engine.sync()
            assertNotNull(copy.numbers(input("total", "2026-11-01")))
            assertNull(copy.numbers(input("total", "2026-11-02")))
        }

    @Test
    fun aKindOfItemThePhoneDoesNotKnowIsOnePlainLineAndTheSyncCompletes() =
        runTest {
            computer.chats["c1"] =
                FakeComputer.FakeChat(
                    "Odd",
                    "2026-11-01T10:00:00Z",
                    listOf(
                        message("user", "Look at this"),
                        buildJsonObject {
                            put("kind", "image")
                            put("time", "2026-11-01T10:00:00Z")
                            put("url", "SECRET-LINK")
                        },
                        message("system", "a role the phone does not know"),
                        buildJsonObject {
                            put("kind", "message")
                            put("role", "user")
                        },
                        message("assistant", "Seen"),
                    ),
                )

            assertEquals(SyncResult.Done, engine.sync())

            val lines = copy.messages("c1").orEmpty()
            assertEquals(5, lines.size)
            assertEquals(ChatMessage(ChatRole.USER, "Look at this"), lines.first())
            assertEquals(List(3) { ChatMessage(ChatRole.TOOL, UNKNOWN_ITEM_LINE) }, lines.subList(1, 4))
            assertEquals(ChatMessage(ChatRole.ASSISTANT, "Seen"), lines.last())
            // The line of the computer is not kept, and the next check does not read the chat again.
            assertFalse(lines.any { "SECRET-LINK" in it.text })
            computer.requests.clear()
            engine.sync()
            assertEquals(listOf("sync-check"), computer.commands())
        }

    @Test
    fun aFactInAShapeThePhoneDoesNotKnowIsLeftOutAndTheOthersStay() =
        runTest {
            computer.facts =
                listOf(
                    fact("f1"),
                    fact("f2", kind = "mood"),
                    buildJsonObject {
                        put("kind", "fact")
                        put("id", "f3")
                    },
                    fact("f4"),
                )
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(listOf("f1", "f4"), copy.facts().facts.map { it.factId })
        }

    @Test
    fun aFactValueThatIsAnObjectKeepsItsFields() =
        runTest {
            val value =
                buildJsonObject {
                    put("name", "Rust")
                    put("level", 3)
                }
            computer.facts = listOf(fact("f1", value))
            engine.sync()
            assertEquals(
                value,
                copy
                    .facts()
                    .facts
                    .single()
                    .value,
            )
        }

    @Test
    fun theComputerEraseingEverythingEmptiesTheCopyAndThePhoneChatsAndRefillsIt() =
        runTest {
            serveSomething()
            engine.sync()
            phoneChat()
            computer.facts = listOf(fact("fresh"))
            computer.chats.clear()
            computer.numbers = emptyList()
            computer.reset = "marker-2"

            assertEquals(SyncResult.Done, engine.sync())

            assertEquals(listOf("fresh"), copy.facts().facts.map { it.factId })
            assertEquals(emptyList(), copy.conversations())
            assertNull(copy.messages("phone"))
            assertEquals("marker-2", copy.syncRecord().resetMarker)
            assertTrue(copy.syncRecord().firstCopyDone)

            // What the user starts after that sync stays through the next ones.
            phoneChat()
            engine.sync()
            assertEquals(listOf("phone"), copy.conversations().map { it.id })
        }

    @Test
    fun aCopyThatSawNoMarkerKeepsTheCurrentOneAndEmptiesNothing() =
        runTest {
            serveSomething()
            phoneChat()
            engine.sync()
            assertEquals("marker-1", copy.syncRecord().resetMarker)
            assertEquals(3, copy.conversations().size)
            assertNotNull(copy.messages("phone"))
        }

    @Test
    fun theEmptyingHoldsEvenWhenTheRefillIsCut() =
        runTest {
            serveSomething()
            engine.sync()
            phoneChat()
            computer.reset = "marker-2"
            computer.requests.clear()
            computer.lostAfter = 1

            assertEquals(SyncResult.Missed(ComputerLink.Unreachable), engine.sync())

            assertEquals(emptyList(), copy.conversations())
            assertEquals(0, copy.facts().facts.size)
            assertEquals("marker-2", copy.syncRecord().resetMarker)

            computer.lostAfter = null
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(3, copy.facts().facts.size)
        }

    @Test
    fun aSyncThatIsCutLeavesTheCopyAsItWasAndTheNextOneCompletesIt() =
        runTest {
            serveSomething()
            engine.sync()
            val before = snapshot()
            computer.facts += fact("f4")
            computer.chats["c1"]!!.items += message("user", "One more")
            computer.numbers += numbersItem("total", "2026-11-03", 10)

            // Every step of the way: the channel is lost after one request, two, three and so on, until it is not lost.
            var asked = 0
            var result: SyncResult
            do {
                asked++
                computer.lostAfter = asked
                computer.requests.clear()
                result = engine.sync()
                if (result != SyncResult.Done) {
                    assertEquals(SyncResult.Missed(ComputerLink.Unreachable), result, "lost after $asked")
                    assertEquals(before, snapshot(), "the copy was touched when the channel was lost after $asked")
                }
            } while (result != SyncResult.Done)
            assertTrue(asked > 1, "the sync needs more than one request")

            computer.lostAfter = null
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(4, copy.facts().facts.size)
            assertEquals(3, copy.messages("c1")?.size)
            assertNotNull(copy.numbers(input("total", "2026-11-03")))
        }

    @Test
    fun theFirstCopyThatIsCutIsNotCompleteAndTheNextOneMakesItWhole() =
        runTest {
            serveSomething()
            computer.lostAfter = 4

            assertEquals(SyncResult.Missed(ComputerLink.Unreachable), engine.sync())
            assertFalse(copy.syncRecord().firstCopyDone)
            assertEquals(0, copy.facts().facts.size)
            assertEquals(emptyList(), copy.conversations())

            computer.lostAfter = null
            assertEquals(SyncResult.Done, engine.sync())
            assertTrue(copy.syncRecord().firstCopyDone)
            assertEquals(3, copy.facts().facts.size)
        }

    @Test
    fun aPartThatChangesWhileItsPagesAreReadIsReadAgainAndNeverMixed() =
        runTest {
            computer.facts = listOf(fact("f1"), fact("f2"), fact("f3"), fact("f4"), fact("f5"))
            var changed = false
            computer.beforeAnswer = { command ->
                if (command == "sync-facts" && computer.requests.count {
                        it["cmd"] ==
                            JsonPrimitive(
                                "sync-facts",
                            )
                    } == 2 &&
                    !changed
                ) {
                    changed = true
                    computer.facts = listOf(fact("g1"), fact("g2"), fact("g3"))
                }
            }

            assertEquals(SyncResult.Done, engine.sync())

            assertEquals(listOf("g1", "g2", "g3"), copy.facts().facts.map { it.factId })
        }

    @Test
    fun aComputerThatIsErasingCutsTheSyncAndTheCopyStaysAsItWas() =
        runTest {
            serveSomething()
            engine.sync()
            val before = snapshot()
            computer.facts += fact("f4")
            computer.erasing = true

            assertEquals(SyncResult.Cut, engine.sync())
            assertEquals(before, snapshot())

            computer.erasing = false
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(4, copy.facts().facts.size)
        }

    @Test
    fun aPartThatCannotBeReadNowIsLeftAndTheOthersAreCopied() =
        runTest {
            serveSomething()
            computer.failing = setOf("sync-document")
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(3, copy.facts().facts.size)
            assertEquals(2, copy.conversations().size)
            assertNull(copy.syncRecord().dashboardToken)
            assertTrue(copy.syncRecord().firstCopyDone)

            computer.failing = emptySet()
            engine.sync()
            assertNotNull(copy.syncRecord().dashboardToken)
            assertNotNull(copy.numbers(input("total", "2026-11-01")))
        }

    @Test
    fun whyTheComputerCouldNotBeReachedIsPassedOnAndTheCopyIsAsItWas() =
        runTest {
            serveSomething()
            engine.sync()
            val before = snapshot()
            for (link in listOf(
                ComputerLink.NoNetwork,
                ComputerLink.NotPaired,
                ComputerLink.PhoneUpdate,
                ComputerLink.ComputerUpdate,
            )) {
                computer.missed = link
                assertEquals(SyncResult.Missed(link), engine.sync())
                assertEquals(before, snapshot())
            }
        }

    @Test
    fun aCopyThatBelongsToAnotherComputerIsEmptiedBeforeAnythingIsReadIntoIt() =
        runTest {
            serveSomething()
            engine.sync()
            phoneChat()
            val other = FakeComputer("computer-b")
            other.facts = listOf(fact("b1"))
            other.reset = "other-marker"

            assertEquals(SyncResult.Done, SyncEngine(other, copy, now = { "2026-11-11T12:00:00Z" }).sync())

            assertEquals(listOf("b1"), copy.facts().facts.map { it.factId })
            assertEquals(listOf("phone"), copy.conversations().map { it.id })
            assertEquals("computer-b", copy.syncRecord().computer)
            assertEquals("other-marker", copy.syncRecord().resetMarker)
        }

    @Test
    fun theSameComputerOnANewAddressKeepsTheCopyAndReadsNothingAgain() =
        runTest {
            serveSomething()
            engine.sync()
            computer.requests.clear()

            copy.adoptComputer(computer.identity)
            engine.sync()

            assertEquals(3, copy.facts().facts.size)
            assertEquals(listOf("sync-check"), computer.commands())
        }

    @Test
    fun aSyncForAComputerThatIsNotTheCopysOwnAnymoreIsRefusedWhole() =
        runTest {
            serveSomething()
            computer.beforeAnswer = { if (it == "sync-dashboard") copy.adoptComputer("computer-b") }

            assertEquals(SyncResult.Cut, engine.sync())

            assertEquals(0, copy.facts().facts.size)
            assertEquals(emptyList(), copy.conversations())
            assertEquals("computer-b", copy.syncRecord().computer)
        }

    @Test
    fun theDashboardDocumentComesInThePhonesTypesAndWhatItCannotHoldIsLeftOut() =
        runTest {
            computer.document =
                Json.parseToJsonElement(
                    """
                    {"type": "x", "title": "Activity", "payload": {"period": "last_7_days", "sessions": "own", "source": "ai_sessions",
                      "widgets": [
                        {"id": "total", "kind": "total", "position": {"column": 0, "row": 0, "width": 12, "height": 2}},
                        {"id": "apps", "kind": "table", "group_by": "app", "source": "on_screen",
                         "position": {"column": 0, "row": 2, "width": 6, "height": 5}, "filter": {"projects": ["a"]}},
                        {"id": "future", "kind": "hologram", "position": {"column": 0, "row": 9, "width": 1, "height": 1}}
                      ], "hidden_projects": [], "project_names": {}}}
                    """.trimIndent(),
                ) as JsonObject
            engine.sync()

            val document = copy.document().document
            assertNotNull(document)
            assertEquals("Activity", document.title)
            assertEquals(Period.LAST_7_DAYS, document.period)
            assertEquals(Source.AI_SESSIONS, document.source)
            assertEquals(listOf("total", "apps"), document.widgets.map { it.id })
            assertEquals(WidgetKind.TABLE, document.widgets.last().kind)
            assertEquals(GroupBy.APP, document.widgets.last().groupBy)
            assertEquals(Source.ON_SCREEN, document.widgets.last().source)
            assertEquals(
                listOf("a"),
                document.widgets
                    .last()
                    .filter
                    ?.projects,
            )
        }

    @Test
    fun aPhoneThatPairsAgainWithTheSameComputerKeepsItsOwnChatsWhenTheComputerHasANewMarker() =
        runTest {
            serveSomething()
            engine.sync()
            phoneChat()
            // The computer dropped its marker when it unpaired the phone and made a new one for the new pairing.
            computer.reset = "marker-of-the-new-pairing"

            copy.adoptComputer(computer.identity)

            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(3, copy.facts().facts.size)
            assertEquals(listOf("c1", "c2", "phone"), copy.conversations().map { it.id }.sorted())
            assertNotNull(copy.messages("phone"))
            assertEquals("marker-of-the-new-pairing", copy.syncRecord().resetMarker)
            assertTrue(copy.syncRecord().firstCopyDone)
        }

    @Test
    fun aStoreThatCannotBeReadCutsTheSyncBeforeAnyDataIsAskedForAndTheNextOneGoesOn() =
        runTest {
            serveSomething()
            failing.failWhen = { "reset_marker, facts_token" in it }

            val result = engine.sync()

            assertEquals(SyncResult.Cut, result)
            assertEquals(emptyList(), computer.dataCommands())

            failing.failWhen = { false }
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(3, copy.facts().facts.size)
        }

    @Test
    fun aStoreThatCannotListTheChatsItHoldsCutsTheSyncAndWritesNothing() =
        runTest {
            serveSomething()
            failing.failWhen = { "AS items" in it }

            assertEquals(SyncResult.Cut, engine.sync())

            assertEquals(0, copy.facts().facts.size)
            assertEquals(emptyList(), copy.conversations())
            assertFalse(copy.syncRecord().firstCopyDone)

            failing.failWhen = { false }
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(2, copy.conversations().size)
        }

    @Test
    fun aStoreThatFailsWhileTheCopyIsEmptiedLeavesItAsItWasAndTheNextSyncEmptiesItWhole() =
        runTest {
            serveSomething()
            engine.sync()
            phoneChat()
            val before = snapshot()
            computer.facts = listOf(fact("fresh"))
            computer.reset = "marker-2"
            failing.failWhen = { it.startsWith("DELETE FROM conversation") }

            assertEquals(SyncResult.Cut, engine.sync())
            assertEquals(before, snapshot())

            failing.failWhen = { false }
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(listOf("fresh"), copy.facts().facts.map { it.factId })
            assertEquals(listOf("c1", "c2"), copy.conversations().map { it.id }.sorted())
        }

    @Test
    fun aStoreThatFailsAtTheEndOfASyncLeavesFactsChatsNumbersAndTokensAsTheyWere() =
        runTest {
            serveSomething()
            engine.sync()
            val before = snapshot()
            computer.facts += fact("f4")
            computer.chats["c1"]!!.items += message("user", "One more")
            computer.chats["c3"] =
                FakeComputer.FakeChat("Third", "2026-11-03T10:00:00Z", listOf(message("user", "New")))
            computer.numbers += numbersItem("total", "2026-11-03", 10)
            // The last statement of a sync: everything before it has been written inside the transaction.
            failing.failWhen = { it.startsWith("UPDATE sync SET first_copy_done") }

            val result = engine.sync()

            assertEquals(SyncResult.Cut, result)
            assertEquals(before, snapshot())

            failing.failWhen = { false }
            assertEquals(SyncResult.Done, engine.sync())
            assertEquals(4, copy.facts().facts.size)
            assertEquals(3, copy.messages("c1")?.size)
            assertNotNull(copy.messages("c3"))
            assertNotNull(copy.numbers(input("total", "2026-11-03")))
        }

    @Test
    fun whatTheComputerSaysIsNeverInWhatTheEngineReportsBack() =
        runTest {
            computer.facts = listOf(fact("f1", JsonPrimitive("SECRET-FACT")))
            computer.chats["c1"] =
                FakeComputer.FakeChat("SECRET-TITLE", "2026-11-01T10:00:00Z", listOf(message("user", "SECRET-LINE")))
            computer.failing = setOf("sync-dashboard", "sync-document")
            computer.errorText = "SECRET-ERROR"
            val reported = mutableListOf<String>()

            val result = engine.sync { reported += it.toString() }

            // The copy holds what the computer gave; what the engine says about it is counts and words of its own.
            assertEquals(SyncResult.Done, result)
            assertEquals(listOf("SECRET-FACT"), copy.facts().facts.map { it.value.jsonPrimitive.content })
            val said = reported + result.toString()
            assertTrue(said.isNotEmpty())
            assertFalse(said.any { "SECRET" in it }, said.toString())
            assertEquals(null, copy.syncRecord().dashboardToken, "a part the computer cannot give is read again later")
        }

    /** Everything a reader of the copy can see, to compare before and after a sync that must change nothing. */
    private fun snapshot() =
        Snapshot(
            copy.facts(),
            copy.conversations(),
            copy.conversations().map { it.id to copy.messages(it.id) },
            copy.document(),
            copy.syncRecord(),
            driver
                .executeQuery(
                    null,
                    "SELECT input_json || numbers_json FROM numbers ORDER BY input_json",
                    { cursor ->
                        val rows = mutableListOf<String>()
                        while (cursor.next().value) rows += cursor.getString(0).orEmpty()
                        QueryResult.Value(rows)
                    },
                    0,
                ).value,
        )

    private data class Snapshot(
        val facts: Any,
        val chats: Any,
        val messages: List<Pair<String, List<ChatMessage>?>>,
        val document: Any,
        val record: Any,
        val numbers: List<String>,
    )

    /** A store that fails the way a full disk or a damaged file does, for the statements the test names. */
    private class FailingDriver(
        private val inner: SqlDriver,
    ) : SqlDriver by inner {
        var failWhen: (String) -> Boolean = { false }

        override fun <R> executeQuery(
            identifier: Int?,
            sql: String,
            mapper: (SqlCursor) -> QueryResult<R>,
            parameters: Int,
            binders: (SqlPreparedStatement.() -> Unit)?,
        ): QueryResult<R> {
            check(!failWhen(sql)) { "SECRET-DISK could not read" }
            return inner.executeQuery(identifier, sql, mapper, parameters, binders)
        }

        override fun execute(
            identifier: Int?,
            sql: String,
            parameters: Int,
            binders: (SqlPreparedStatement.() -> Unit)?,
        ): QueryResult<Long> {
            check(!failWhen(sql)) { "SECRET-DISK could not write" }
            return inner.execute(identifier, sql, parameters, binders)
        }
    }
}
