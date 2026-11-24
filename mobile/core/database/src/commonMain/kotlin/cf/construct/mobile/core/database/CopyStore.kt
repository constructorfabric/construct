package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationOrigin
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DocumentProblem
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlin.time.Instant

/**
 * The phone's copy of the conversations, the facts and the dashboard, in SQLite. It has one method
 * per read, write and delete of the copy. A caller that must not block a thread runs these calls on
 * its own dispatcher.
 *
 * Structured values are kept as JSON text in the shape the types of `core/model` have today, and
 * enums by their names. A key that a type no longer has is ignored when a row is read, so removing a
 * field needs no migration. Adding a required field or renaming a field or a constant changes what
 * stored rows must look like, so it needs a migration that rewrites them, as a column change does.
 * A row that no longer reads fails the read, and the reads of [DatabaseConstruct] answer it as a
 * failure.
 */
@Suppress("TooManyFunctions")
class CopyStore(
    driver: SqlDriver,
) {
    private val queries = ConstructDatabase(driver).copyQueries

    fun conversations(): List<ConversationSummary> {
        val query =
            queries.listConversations { id, title, time, origin, preview ->
                val source = if (origin == PHONE_ORIGIN) ConversationOrigin.PHONE else ConversationOrigin.COMPUTER
                ConversationSummary(id, title, time, source, preview)
            }
        return query
            .executeAsList()
            .map { it to runCatching { Instant.parse(it.time) }.getOrNull() }
            .sortedWith(compareByDescending<Pair<ConversationSummary, Instant?>> { it.second }.thenBy { it.first.id })
            .map { it.first }
    }

    internal fun conversationOrigin(conversation: String): String? =
        queries.conversationOrigin(conversation).executeAsOneOrNull()

    internal fun chatRecords(conversation: String): List<String> = queries.recordsOf(conversation).executeAsList()

    /** A native record and its visible message, if any, are committed together. */
    internal fun appendPhoneRecord(
        record: PhoneRecord,
        position: Long,
        line: String,
    ) = queries.transaction {
        when (conversationOrigin(record.session)) {
            null -> {
                val message = checkNotNull(record.message)
                check(message.role == ChatRole.USER)
                val title = conversationTitle(message.text)
                queries.saveConversation(record.session, title, record.time)
                queries.setConversationOrigin(PHONE_ORIGIN, record.session)
            }

            PHONE_ORIGIN -> {
                queries.updateConversationTime(record.time, record.session)
            }

            else -> {
                error(COMPUTER_READ_ONLY)
            }
        }
        queries.saveRecord(record.session, position, line)
        record.message?.let { queries.saveMessage(record.session, position, it.role.name, it.text) }
    }

    internal fun deletePhoneConversations(which: String): Outcome<Unit> =
        queries.transactionWithResult {
            when {
                which == DELETE_ALL_CONVERSATIONS -> {
                    queries.phoneConversations().executeAsList().forEach(::removeConversation)
                }

                conversationOrigin(which) == PHONE_ORIGIN -> {
                    removeConversation(which)
                }

                conversationOrigin(which) != null -> {
                    return@transactionWithResult Outcome.Failed(COMPUTER_READ_ONLY)
                }
            }
            Outcome.Ok(Unit)
        }

    private fun removeConversation(id: String) {
        queries.deleteRecordsOf(id)
        queries.deleteMessagesOf(id)
        queries.deleteConversation(id)
    }

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
        queries.updateConversation(conversation.title, conversation.time, conversation.id)
        messages.forEachIndexed { index, message ->
            queries.saveMessage(conversation.id, index.toLong(), message.role.name, message.text)
        }
    }

    /** One conversation, or [cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS]. */
    fun deleteConversation(which: String) =
        queries.transaction {
            if (which == DELETE_ALL_CONVERSATIONS) {
                queries.deleteAllRecords()
                queries.deleteAllMessages()
                queries.deleteAllConversations()
            } else {
                removeConversation(which)
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

    /** Removes the computer's copy while preserving phone chats and their native records. */
    fun removeComputerCopy() =
        queries.transaction {
            queries.computerConversations().executeAsList().forEach(::removeConversation)
            removeComputerData()
        }

    /** Empties the conversations, the facts, the dashboard and the record of the sync. */
    fun deleteEverything() =
        queries.transaction {
            queries.deleteAllRecords()
            queries.deleteAllMessages()
            queries.deleteAllConversations()
            removeComputerData()
        }

    private fun removeComputerData() {
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
        const val TITLE_LENGTH = 60

        fun conversationTitle(text: String): String {
            var end = 0
            repeat(TITLE_LENGTH) {
                if (end == text.length) return text
                val paired = text[end].isHighSurrogate() && text.getOrNull(end + 1)?.isLowSurrogate() == true
                end += if (paired) 2 else 1
            }
            return text.substring(0, end)
        }

        val json = Json { ignoreUnknownKeys = true }
    }
}
