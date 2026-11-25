package cf.construct.mobile.core.sync

import app.cash.sqldelight.db.QueryResult
import app.cash.sqldelight.driver.jdbc.sqlite.JdbcSqliteDriver
import cf.construct.mobile.core.connection.ComputerAccess
import cf.construct.mobile.core.connection.ComputerChannel
import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.ComputerReach
import cf.construct.mobile.core.connection.PairResult
import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.core.connection.phoneConnectionOver
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.sql.ConstructDatabase
import cf.construct.mobile.core.keys.fakePairingSecret
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The sync against the agent's own test program, a real process that speaks the real protocol over real TLS: what the
 * phone's engine reads is what the agent serves. It runs when `CONSTRUCT_PHONE_PAIRING_EXAMPLE` names the built
 * program (`cargo build -p construct-agent --example phone_pairing`), and is skipped otherwise.
 */
class RealAgentSyncTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val binary = System.getenv("CONSTRUCT_PHONE_PAIRING_EXAMPLE")
    private val started = mutableListOf<RealAgent>()
    private val driver = JdbcSqliteDriver(JdbcSqliteDriver.IN_MEMORY).also { ConstructDatabase.Schema.create(it) }
    private val copy = CopyStore(driver)

    /** What a phone keeps of its pairing: shared by the connections of one test, as the app has one. */
    private val secret = fakePairingSecret { false }

    /** The commands each sync asked, to see what a sync read. */
    private val asked = mutableListOf<String>()
    private lateinit var phone: PhoneConnection
    private lateinit var engine: SyncEngine

    @Before
    fun setUp() {
        assumeTrue("CONSTRUCT_PHONE_PAIRING_EXAMPLE is not set", binary != null)
        phone = connection()
        engine = SyncEngine(counting(phone), copy)
    }

    @After
    fun tearDown() = started.forEach { it.stop() }

    private fun connection(version: Int = 1): PhoneConnection =
        phoneConnectionOver(
            JvmPhoneTransport(),
            secret,
            networkAvailable = { true },
            protocolVersion = version,
            adoptComputer = copy::adoptComputer,
        )

    private fun counting(inner: ComputerAccess) =
        object : ComputerAccess {
            override suspend fun <T> reach(block: (ComputerChannel) -> T): ComputerReach<T> =
                inner.reach { channel ->
                    block(
                        object : ComputerChannel by channel {
                            override fun ask(request: JsonObject): JsonObject {
                                asked += request["cmd"]!!.jsonPrimitive.content
                                return channel.ask(request)
                            }
                        },
                    )
                }
        }

    private fun start(
        name: String,
        data: MadeUpData,
        port: Int = RealAgent.freePort(),
        dir: java.io.File = folder.newFolder(name),
    ) = RealAgent(binary!!, dir, data.file, port).also { started += it }

    private fun data(name: String) = MadeUpData(folder.newFile("$name.json"))

    private fun pair(agent: RealAgent) = runBlocking { phone.pairByQr(agent.payload, "Test phone") }

    private fun sync() = runBlocking { engine.sync() }

    /** Every answer of the numbers the copy holds, with the input it was counted for. */
    private fun storedNumbers(): List<Pair<DashboardInput, DashboardNumbers>> =
        driver
            .executeQuery(
                null,
                "SELECT input_json, numbers_json FROM numbers",
                { cursor ->
                    val rows = mutableListOf<Pair<DashboardInput, DashboardNumbers>>()
                    while (cursor.next().value) {
                        rows +=
                            Json.decodeFromString(DashboardInput.serializer(), cursor.getString(0)!!) to
                            Json.decodeFromString(DashboardNumbers.serializer(), cursor.getString(1)!!)
                    }
                    QueryResult.Value(rows)
                },
                0,
            ).value

    private fun someData(name: String) =
        data(name).apply {
            facts = listOf(MadeUpData.fact("f1"), MadeUpData.fact("f2", buildJsonObject { put("name", "Rust") }))
            chats =
                listOf(
                    MadeUpData.chat(
                        "c1",
                        "Plan",
                        buildJsonObject {
                            put("kind", "message")
                            put("role", "user")
                            put("time", "2026-11-01T10:00:00Z")
                            put("text", "Plan my week")
                        },
                        buildJsonObject {
                            put("kind", "tool")
                            put("time", "2026-11-01T10:00:01Z")
                            put("line", "Looked up your profile")
                        },
                        buildJsonObject {
                            put("kind", "message")
                            put("role", "assistant")
                            put("time", "2026-11-01T10:00:02Z")
                            put("text", "Start on Monday.")
                        },
                    ),
                )
            sessions =
                listOf(
                    MadeUpData.session("app", "2026-11-02T10:00:00Z", "2026-11-02T10:01:00Z", "2026-11-02T10:02:00Z"),
                )
            write()
        }

    @Test
    fun theFirstCopyGivesTheFactsTheChatsAndTheNumbersTheAgentServes() {
        val agent = start("a", someData("a"))
        assertEquals(PairResult.Paired, pair(agent))
        val progress = mutableListOf<StepProgress>()

        assertEquals(SyncResult.Done, runBlocking { engine.sync { progress += it } })

        assertEquals(listOf("f1", "f2"), copy.facts().facts.map { it.factId })
        assertEquals(
            listOf(ChatRole.USER, ChatRole.TOOL, ChatRole.ASSISTANT),
            copy.messages("c1")?.map { it.role },
        )
        assertEquals("Looked up your profile", copy.messages("c1")?.get(1)?.text)
        assertEquals(ConversationOrigin.COMPUTER, copy.conversations().single().origin)
        val document = copy.document().document
        assertNotNull(document, "the agent's default dashboard")
        assertEquals(
            listOf("total", "per-day", "by-project", "by-ticket", "end-of-day"),
            document.widgets.map { it.id },
        )
        assertEquals(WidgetKind.TABLE, document.widgets[2].kind)
        val counted = storedNumbers().filter { (input, _) -> (input as? DashboardInput.ForWidget)?.widget == "total" }
        assertTrue(counted.isNotEmpty(), "the numbers of the days with activity")
        assertTrue(counted.any { (_, numbers) -> numbers.totalMinutes > 0 })
        // A week of the table by project, as the program counts it: seven days and one group.
        val week =
            storedNumbers()
                .filter { (input, _) -> (input as? DashboardInput.ForWidget)?.widget == "by-project" }
                .map { (_, numbers) -> numbers }
                .single { it.days.size == 7 }
        assertEquals(3, week.totalMinutes)
        assertEquals(listOf("app"), week.groups.map { it.key })
        assertEquals(GroupMinutes("app", 3, 1.0, 1), week.groups.single())
        assertEquals("UTC", copy.syncRecord().zone)
        assertTrue(copy.syncRecord().firstCopyDone)
        assertEquals(listOf(SyncStep.MEMORY, SyncStep.CHATS, SyncStep.DASHBOARD), progress.map { it.step }.distinct())
        assertTrue(progress.all { it.copied >= 0 })
    }

    @Test
    fun aCheckThatFindsNothingNewAsksForNoData() {
        val agent = start("a", someData("a"))
        pair(agent)
        sync()
        asked.clear()

        assertEquals(SyncResult.Done, sync())

        assertEquals(listOf("sync-check"), asked)
    }

    @Test
    fun aNewFactAChatAndAMessageComeAndADeletedFactAndChatGo() {
        val data = someData("a")
        val agent = start("a", data)
        pair(agent)
        sync()

        data.facts = listOf(MadeUpData.fact("f2", buildJsonObject { put("name", "Rust") }), MadeUpData.fact("f3"))
        val first = data.chats.single()
        val more =
            buildJsonObject {
                put("kind", "message")
                put("role", "user")
                put("time", "2026-11-02T09:00:00Z")
                put("text", "And Tuesday?")
            }
        val grown = (first["items"] as JsonArray).map { it as JsonObject } + more
        data.chats = listOf(MadeUpData.chatOf("c1", "Plan", grown), MadeUpData.chat("c2", "New chat", more))
        data.write()
        asked.clear()
        assertEquals(SyncResult.Done, sync())

        assertEquals(listOf("f2", "f3"), copy.facts().facts.map { it.factId })
        assertEquals(4, copy.messages("c1")?.size)
        assertEquals("And Tuesday?", copy.messages("c2")?.single()?.text)
        assertFalse("sync-dashboard" in asked, "the numbers did not change")

        data.chats = listOf(MadeUpData.chat("c2", "New chat", more))
        data.write()
        sync()
        assertNull(copy.messages("c1"))
    }

    @Test
    fun theComputerErasingEverythingEmptiesThePhoneEvenTheChatsItStartedAndWhatItStartsAfterStays() {
        val data = someData("a")
        val agent = start("a", data)
        pair(agent)
        sync()
        driver.execute(null, "INSERT INTO conversation VALUES ('phone', 'Mine', '2026-11-04T10:00:00Z', 'phone')", 0)
        driver.execute(null, "INSERT INTO message VALUES ('phone', 0, 'USER', 'Mine')", 0)

        data.facts = emptyList()
        data.chats = emptyList()
        data.sessions = emptyList()
        data.write()
        assertEquals("erased", agent.control("erase"))
        assertEquals(SyncResult.Done, sync())

        assertEquals(0, copy.facts().facts.size)
        assertEquals(emptyList(), copy.conversations())
        assertEquals(emptyList(), storedNumbers())

        driver.execute(null, "INSERT INTO conversation VALUES ('phone2', 'After', '2026-11-05T10:00:00Z', 'phone')", 0)
        sync()
        assertEquals(listOf("phone2"), copy.conversations().map { it.id })
    }

    @Test
    fun aSyncThatIsCutByTheProgramStoppingLeavesTheCopyAsItWasAndTheNextOneCompletes() {
        val dir = folder.newFolder("a")
        val port = RealAgent.freePort()
        val data = someData("a")
        val agent = start("a", data, port, dir)
        pair(agent)
        sync()
        val before = copy.facts().facts

        data.facts += MadeUpData.fact("f9")
        data.write()
        // The program is stopped after the check has been answered, as a computer that goes to sleep in the middle.
        val cut = SyncEngine(stoppingAfter(phone, agent, "sync-check"), copy)
        assertEquals(SyncResult.Missed(ComputerLink.Unreachable), runBlocking { cut.sync() })
        assertEquals(before, copy.facts().facts)

        val again = start("a2", data, port, dir)
        // The same computer on the same address and port: the phone connects with what it holds.
        assertEquals(SyncResult.Done, sync())
        assertEquals(3, copy.facts().facts.size)
        assertNotNull(again)
    }

    private fun stoppingAfter(
        inner: ComputerAccess,
        agent: RealAgent,
        command: String,
    ) = object : ComputerAccess {
        override suspend fun <T> reach(block: (ComputerChannel) -> T): ComputerReach<T> =
            inner.reach { channel ->
                block(
                    object : ComputerChannel by channel {
                        override fun ask(request: JsonObject): JsonObject {
                            val answer = channel.ask(request)
                            if (request["cmd"] == JsonPrimitive(command)) agent.stop()
                            return answer
                        }
                    },
                )
            }
    }

    @Test
    fun theSameComputerOnANewPortKeepsTheCopyAndAnotherComputerEmptiesIt() {
        val dirA = folder.newFolder("a")
        val a = someData("a")
        val first = start("a", a, dir = dirA)
        assertEquals(PairResult.Paired, pair(first))
        sync()
        driver.execute(null, "INSERT INTO conversation VALUES ('phone', 'Mine', '2026-11-04T10:00:00Z', 'phone')", 0)
        first.stop()

        // The same computer, restarted on another port: pairing again keeps the copy, and shows no loading screen.
        val moved = start("a-moved", a, dir = dirA)
        assertEquals(PairResult.Paired, pair(moved))
        assertEquals(listOf("f1", "f2"), copy.facts().facts.map { it.factId })
        assertTrue(copy.syncRecord().firstCopyDone)
        asked.clear()
        assertEquals(SyncResult.Done, sync())
        assertEquals(listOf("sync-check"), asked)

        // Another computer: from the moment the pairing completes, nothing of the old one is there.
        val b =
            data("b").apply {
                facts = listOf(MadeUpData.fact("b1"))
                write()
            }
        val other = start("b", b)
        assertEquals(PairResult.Paired, pair(other))
        assertEquals(0, copy.facts().facts.size)
        assertEquals(listOf("phone"), copy.conversations().map { it.id })
        assertNull(copy.document().document)
        assertEquals(emptyList(), storedNumbers())
        assertFalse(copy.syncRecord().firstCopyDone)
        assertEquals(SyncResult.Done, sync())
        assertEquals(listOf("b1"), copy.facts().facts.map { it.factId })
        assertEquals(listOf("phone"), copy.conversations().map { it.id })

        // And the other way round: the first computer again.
        val back = start("a-again", a, dir = dirA)
        assertEquals(PairResult.Paired, pair(back))
        assertEquals(0, copy.facts().facts.size)
        sync()
        assertEquals(listOf("f1", "f2"), copy.facts().facts.map { it.factId })
    }

    @Test
    fun aPhoneThatPairsAgainAfterTheComputerUnpairedItKeepsTheChatsItStarted() {
        val dir = folder.newFolder("a")
        val data = someData("a")
        val first = start("a", data, dir = dir)
        pair(first)
        sync()
        val marker = copy.syncRecord().resetMarker
        assertNotNull(marker)
        driver.execute(null, "INSERT INTO conversation VALUES ('phone', 'Mine', '2026-11-04T10:00:00Z', 'phone')", 0)
        driver.execute(null, "INSERT INTO message VALUES ('phone', 0, 'USER', 'Mine')", 0)
        assertEquals("unpaired", first.control("unpair"))
        first.stop()

        // The computer drops its marker with its last phone and makes a new one for the next pairing.
        val again = start("a-again", data, dir = dir)
        assertEquals(PairResult.Paired, pair(again))
        assertEquals(SyncResult.Done, sync())

        assertNotEquals(marker, copy.syncRecord().resetMarker, "the computer made a new marker for the new pairing")
        assertEquals(listOf("c1", "phone"), copy.conversations().map { it.id }.sorted())
        assertEquals(listOf("f1", "f2"), copy.facts().facts.map { it.factId })
    }

    @Test
    fun aPhoneThatTheComputerUnpairedKeepsItsCopy() {
        val agent = start("a", someData("a"))
        pair(agent)
        sync()
        assertEquals("unpaired", agent.control("unpair"))

        assertEquals(SyncResult.Missed(ComputerLink.Unreachable), sync())

        assertEquals(2, copy.facts().facts.size)
        assertEquals(1, copy.conversations().size)
    }

    @Test
    fun aKindOfItemThePhoneDoesNotKnowIsOnePlainLineAndTheSyncCompletes() {
        val data = someData("a")
        data.chats =
            listOf(
                MadeUpData.chat(
                    "c1",
                    "Odd",
                    buildJsonObject {
                        put("kind", "message")
                        put("role", "user")
                        put("time", "2026-11-01T10:00:00Z")
                        put("text", "Look")
                    },
                    buildJsonObject {
                        put("kind", "image")
                        put("time", "2026-11-01T10:00:01Z")
                        put("url", "x")
                    },
                ),
            )
        data.write()
        val agent = start("a", data)
        pair(agent)

        assertEquals(SyncResult.Done, sync())

        assertEquals(listOf("Look", UNKNOWN_ITEM_LINE), copy.messages("c1")?.map { it.text })
    }

    @Test
    fun aPhoneThatIsTooOldForTheComputerIsAskedToUpdate() {
        val agent = start("a", someData("a"))
        assertEquals(PairResult.Paired, pair(agent))

        val old = SyncEngine(connection(version = 0), copy)

        assertEquals(SyncResult.Missed(ComputerLink.PhoneUpdate), runBlocking { old.sync() })
        assertEquals(SyncResult.Done, sync())
    }
}
