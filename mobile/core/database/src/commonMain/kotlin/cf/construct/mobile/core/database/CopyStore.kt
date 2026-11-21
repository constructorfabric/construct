package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DocumentProblem
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

/**
 * The phone's copy of the conversations, the facts and the dashboard, in SQLite. It has one method
 * per read, write and delete of the copy. A caller that must not block a thread runs these calls on
 * its own dispatcher.
 *
 * Structured values are kept as JSON text in the shape the types of `core/model` have today, and
 * enums by their names. Changing one of those types (a required field, a renamed constant) changes
 * what stored rows must look like, so it needs a migration that rewrites them, as a column change
 * does. A row that no longer reads fails the read, and the reads of [DatabaseConstruct] answer it as
 * a failure.
 */
@Suppress("TooManyFunctions")
class CopyStore(
    driver: SqlDriver,
) {
    private val queries = ConstructDatabase(driver).copyQueries

    fun conversations(): List<ConversationSummary> =
        queries.listConversations { id, title, time -> ConversationSummary(id, title, time) }.executeAsList()

    /** The messages of a conversation, or null when the copy has no such conversation. */
    fun messages(conversation: String): List<ChatMessage>? =
        queries.transactionWithResult {
            if (queries.conversationExists(conversation).executeAsOne() == 0L) {
                null
            } else {
                queries
                    .messagesOf(
                        conversation,
                    ) { role, text -> ChatMessage(ChatRole.valueOf(role), text) }
                    .executeAsList()
            }
        }

    /** Stores a conversation with its messages; what the copy held for that id is replaced. */
    internal fun saveConversation(
        conversation: ConversationSummary,
        messages: List<ChatMessage>,
    ) = queries.transaction {
        queries.deleteMessagesOf(conversation.id)
        queries.saveConversation(conversation.id, conversation.title, conversation.time)
        messages.forEachIndexed { index, message ->
            queries.saveMessage(conversation.id, index.toLong(), message.role.name, message.text)
        }
    }

    /** One conversation, or [cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS]. */
    fun deleteConversation(which: String) =
        queries.transaction {
            if (which == cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS) {
                queries.deleteAllMessages()
                queries.deleteAllConversations()
            } else {
                queries.deleteMessagesOf(which)
                queries.deleteConversation(which)
            }
        }

    fun facts(): MemoryFacts =
        queries.transactionWithResult {
            val facts =
                queries
                    .listFacts { id, kind, property, value, storedAt, tool ->
                        MemoryFact(
                            id,
                            EntityKind.valueOf(kind),
                            property,
                            json.decodeFromString(JsonElement.serializer(), value),
                            storedAt,
                            tool,
                        )
                    }.executeAsList()
            MemoryFacts(facts, memoryVersion())
        }

    fun memoryVersion(): Long? = queries.memoryVersion().executeAsOneOrNull()?.memory_version

    /** Replaces every fact and the profile's version. */
    internal fun saveFacts(facts: MemoryFacts) =
        queries.transaction {
            queries.deleteAllFacts()
            facts.facts.forEach {
                queries.saveFact(
                    it.factId,
                    it.entityKind.name,
                    it.property,
                    json.encodeToString(JsonElement.serializer(), it.value),
                    it.storedAt,
                    it.tool,
                )
            }
            queries.startSync()
            queries.saveMemoryVersion(facts.version)
        }

    /** Deleting a fact that is not there is not an error. */
    fun deleteFact(factId: String) = queries.deleteFact(factId)

    /** Empties the conversations, the facts, the dashboard and the record of the sync. */
    fun deleteEverything() =
        queries.transaction {
            queries.deleteAllMessages()
            queries.deleteAllConversations()
            queries.deleteAllFacts()
            queries.deleteDocument()
            queries.deleteAllNumbers()
            queries.deleteSync()
        }

    fun document(): DocumentState =
        queries
            .documentRow { document, problem ->
                DocumentState(
                    document?.let { json.decodeFromString(DashboardDocument.serializer(), it) },
                    problem?.let { json.decodeFromString(DocumentProblem.serializer(), it) },
                )
            }.executeAsOneOrNull() ?: DocumentState(null, null)

    internal fun saveDocument(state: DocumentState) =
        queries.saveDocument(
            state.document?.let { json.encodeToString(DashboardDocument.serializer(), it) },
            state.problem?.let { json.encodeToString(DocumentProblem.serializer(), it) },
        )

    /** The numbers stored for exactly this input, or null. */
    fun numbers(input: DashboardInput): DashboardNumbers? =
        queries
            .numbersFor(inputKey(input))
            .executeAsOneOrNull()
            ?.let { json.decodeFromString(DashboardNumbers.serializer(), it) }

    internal fun saveNumbers(
        input: DashboardInput,
        numbers: DashboardNumbers,
    ) = queries.saveNumbers(inputKey(input), json.encodeToString(DashboardNumbers.serializer(), numbers))

    /** When the copy was last brought up to date (RFC 3339), or null if it never was. */
    internal fun lastSync(): String? = queries.lastSync().executeAsOneOrNull()?.synced_at

    internal fun markSynced(at: String) =
        queries.transaction {
            queries.startSync()
            queries.saveSyncedAt(at)
        }

    private fun inputKey(input: DashboardInput) = json.encodeToString(DashboardInput.serializer(), input)

    private companion object {
        val json = Json { ignoreUnknownKeys = true }
    }
}
