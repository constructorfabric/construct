package cf.construct.mobile.core.database

import app.cash.sqldelight.driver.jdbc.sqlite.JdbcSqliteDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import kotlinx.serialization.json.JsonPrimitive
import java.io.File
import java.util.Properties
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MigrationTest {
    /** A database file as the first version of the schema made it, with rows in it. */
    private fun versionOneFile(): File {
        val file = File.createTempFile("copy", ".db")
        file.deleteOnExit()
        File("src/commonMain/sqldelight/databases/1.db").copyTo(file, overwrite = true)
        val driver = JdbcSqliteDriver("jdbc:sqlite:${file.path}")
        listOf(
            "INSERT INTO conversation VALUES ('c1', 'Plan my week', '2026-01-01T18:30:00Z')",
            "INSERT INTO message VALUES ('c1', 0, 'USER', 'Plan my week')",
            "INSERT INTO message VALUES ('c1', 1, 'ASSISTANT', 'Start on Monday.')",
            "INSERT INTO fact VALUES ('f1', 'ROLE', 'title', '\"Mobile developer\"', '2026-01-01T08:00:00Z', NULL)",
            "INSERT INTO sync(id, synced_at) VALUES (1, '2026-01-02T10:00:00Z')",
            "PRAGMA user_version = 1",
        ).forEach { driver.execute(null, it, 0) }
        driver.close()
        return file
    }

    @Test
    fun storedRowsSurviveTheMigrationAndTheNewColumnWorks() {
        // Opening a file that holds version 1 with the schema is what upgrades it, as on a phone.
        val driver = JdbcSqliteDriver("jdbc:sqlite:${versionOneFile().path}", Properties(), ConstructDatabase.Schema)
        val store = CopyStore(driver)

        assertEquals(
            listOf(ConversationSummary("c1", "Plan my week", "2026-01-01T18:30:00Z", preview = "Start on Monday.")),
            store.conversations(),
        )
        assertEquals("computer", store.conversationOrigin("c1"))
        assertEquals(emptyList(), store.chatRecords("c1"))
        assertEquals(
            listOf(ChatMessage(ChatRole.USER, "Plan my week"), ChatMessage(ChatRole.ASSISTANT, "Start on Monday.")),
            store.messages("c1"),
        )
        val fact = MemoryFact("f1", EntityKind.ROLE, "title", JsonPrimitive("Mobile developer"), "2026-01-01T08:00:00Z")
        assertEquals(MemoryFacts(listOf(fact), null), store.facts())
        assertEquals("2026-01-02T10:00:00Z", store.lastSync())
        assertNull(store.memoryVersion())

        store.saveFacts(MemoryFacts(listOf(fact), 4L))
        assertEquals(4L, store.memoryVersion())
        driver.close()
    }

    /** A file as the third version made it: a chat of the computer, a chat of the phone and its record, and a sync. */
    private fun versionThreeFile(): File {
        val file = versionOneFile()
        val driver = JdbcSqliteDriver("jdbc:sqlite:${file.path}")
        ConstructDatabase.Schema.migrate(driver, 1, 3)
        listOf(
            "INSERT INTO conversation VALUES ('p1', 'Mine', '2026-01-03T09:00:00Z', 'phone')",
            "INSERT INTO message VALUES ('p1', 0, 'USER', 'Mine')",
            "INSERT INTO chat_record VALUES ('p1', 0, 'a record of the phone')",
            "UPDATE sync SET memory_version = 5",
            "PRAGMA user_version = 3",
        ).forEach { driver.execute(null, it, 0) }
        driver.close()
        return file
    }

    @Test
    fun theFourthVersionKeepsTheChatsStartedOnThePhoneAndStartsWithNoComputer() {
        val driver = JdbcSqliteDriver("jdbc:sqlite:${versionThreeFile().path}", Properties(), ConstructDatabase.Schema)
        val store = CopyStore(driver)

        assertEquals(listOf("p1", "c1"), store.conversations().map { it.id })
        assertEquals("phone", store.conversationOrigin("p1"))
        assertEquals(listOf(ChatMessage(ChatRole.USER, "Mine")), store.messages("p1"))
        assertEquals(listOf("a record of the phone"), store.chatRecords("p1"))
        assertEquals(1, store.facts().facts.size)
        assertEquals(SyncRecord(syncedAt = "2026-01-02T10:00:00Z"), store.syncRecord())
        assertEquals(5L, store.memoryVersion())

        // A copy made before this version has no computer, takes the first one it pairs with and keeps what it holds.
        store.adoptComputer("A")
        assertEquals("A", store.syncRecord().computer)
        assertEquals(1, store.facts().facts.size)
        // After another computer, the phone's chat is still there, and the old copy of the computer is not.
        store.adoptComputer("B")
        assertEquals(listOf("p1"), store.conversations().map { it.id })
        assertTrue(store.facts().facts.isEmpty())
        driver.close()
    }
}
