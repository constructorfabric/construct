package cf.construct.mobile.core.database

import app.cash.sqldelight.coroutines.asFlow
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
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
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

    /**
     * Signals when it is collected, and again after each write to the `fact` table. That covers the
     * profile's version too: it is written only in the transaction that rewrites the facts
     * ([saveFacts], [removeComputerData]), and `markSynced` leaves it alone. A write in a transaction
     * signals when the transaction ends, so a reader that answers the signal sees the whole write.
     */
    internal fun factChanges(): Flow<Unit> = queries.listFacts().asFlow().map { }

    /**
     * Signals when it is collected, and again after each write to the conversations or the lines of any
     * of them: a sync, an answer of the phone's own, a delete. A reader answers the signal by reading the list
     * again, so it sees the whole write.
     */
    internal fun conversationChanges(): Flow<Unit> = queries.listConversations().asFlow().map { }

    /** Signals when it is collected, and again after each write to the dashboard's document or its numbers. */
    internal fun dashboardChanges(): Flow<Unit> = queries.dashboardMarks().asFlow().map { }

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

    /** What the copy remembers about the computer it came from and how far it was brought up to date. */
    fun syncRecord(): SyncRecord =
        queries
            .readSync { computer, marker, facts, chats, dashboard, zone, done, at ->
                SyncRecord(computer, marker, facts, chats, dashboard, zone, done == 1L, at)
            }.executeAsOneOrNull() ?: SyncRecord()

    /** The chats of the computer that the copy holds, with how many lines each has. */
    fun computerChats(): List<ChatOnCopy> =
        queries.computerChats { id, title, items -> ChatOnCopy(id, title, items.toInt()) }.executeAsList()

    /**
     * The copy now belongs to the computer with this identity, because the phone has just paired with it.
     * - The same computer again keeps the copy and its tokens. It forgets the reset marker: a computer drops its
     *   marker when it unpairs its last phone and makes a new one for the next pairing, so a different marker
     *   after a pairing is not an erase. The next check takes the computer's marker as the first it has seen, and
     *   the sync that is applied saves it. An erase that happened while the phone was not paired, or before that
     *   first sync is applied, is not seen as one; the sync still removes what the computer no longer has, and the
     *   chats started on the phone stay.
     * - A copy that has no computer takes it and keeps what it holds.
     * - Another computer empties the copy of what came from the old one (the facts, the chats of the computer, the
     *   dashboard and the record of the sync) in one transaction, so no page reads it half emptied, and the copy then
     *   waits for its first copy from the new one.
     *
     * What the phone started and its model setup stay in every case.
     */
    fun adoptComputer(identity: String) =
        queries.transaction {
            val kept = syncRecord().computer
            if (kept == identity) {
                queries.forgetResetMarker()
            } else {
                if (kept != null) removeCopyOfComputer()
                queries.startSync()
                queries.saveComputer(identity)
            }
        }

    /**
     * The computer erased everything about the person since the copy last saw it: the copy is emptied, the
     * chats started on the phone too, and keeps [marker] as the marker it has seen. The computer it came from
     * and the fact that it was completely copied stay, so the page shows an empty copy and not a loading screen.
     * It is a transaction of its own that commits before the refill, so a refill that is cut leaves the copy empty,
     * as the computer is. An answer that is being written into a chat of the phone at that moment ends as an error,
     * because the chat it continues is gone.
     */
    fun reset(marker: String) =
        queries.transaction {
            val record = syncRecord()
            emptyCopy()
            queries.startSync()
            record.computer?.let(queries::saveComputer)
            if (record.firstCopyDone) queries.saveFirstCopyDone()
            queries.saveResetMarker(marker)
        }

    /**
     * Applies one whole sync in one transaction, so a reader sees the copy as it was or as it is after the sync,
     * never between. A change that the copy already holds is not written again.
     *
     * @throws CopyChanged when the copy belongs to another computer than the update, or when a chat that continues
     * what the copy holds finds another number of lines there.
     */
    fun applySync(update: CopyUpdate) =
        queries.transaction {
            if (syncRecord().computer != update.computer) throw CopyChanged()
            queries.startSync()
            update.facts?.let(::applyFacts)
            update.chats?.let(::applyChats)
            update.dashboard?.let(::applyDashboard)
            update.resetMarker?.let(queries::saveResetMarker)
            queries.saveSyncedAt(update.syncedAt)
            queries.saveFirstCopyDone()
        }

    private fun applyFacts(update: FactsUpdate) {
        val held = facts()
        if (held.facts != update.facts) {
            queries.deleteAllFacts()
            update.facts.forEach {
                queries.saveFact(
                    it.factId,
                    it.entityKind.name,
                    it.property,
                    json.encodeToString(JsonElement.serializer(), it.value),
                    it.storedAt,
                    it.tool,
                )
            }
            // The computer gives no version that fits the stream of the facts, so the copy counts its own: each change
            // is the next version, and that is what makes the stream give the page the new facts. No facts, no version.
            queries.saveMemoryVersion(if (update.facts.isEmpty()) null else (held.version ?: 0L) + 1L)
        }
        queries.saveFactsToken(update.token)
    }

    private fun applyChats(update: ChatsUpdate) {
        val listed = update.chats.map { it.id }.toSet()
        queries
            .computerConversations()
            .executeAsList()
            .filter { it !in listed }
            .forEach(::removeConversation)
        update.chats.forEach { chat ->
            if (conversationOrigin(chat.id) == PHONE_ORIGIN) return@forEach
            if (chat.from == 0) {
                queries.deleteMessagesOf(chat.id)
            } else if (queries.messageCount(chat.id).executeAsOne() != chat.from.toLong()) {
                throw CopyChanged()
            }
            // The first write makes the chat; a title the computer changed is written on every sync, and the time moves
            // only when the chat has a new line (a chat that has none keeps the time it had).
            queries.saveConversation(chat.id, chat.title, chat.time.orEmpty())
            queries.updateConversationTitle(chat.title, chat.id)
            chat.time?.let { queries.updateConversationTime(it, chat.id) }
            chat.messages.forEachIndexed { index, message ->
                queries.saveMessage(chat.id, (chat.from + index).toLong(), message.role.name, message.text)
            }
        }
        queries.saveChatsToken(update.token)
    }

    private fun applyDashboard(update: DashboardUpdate) {
        update.document?.let { saveDocument(DocumentState(it, null)) } ?: queries.deleteDocument()
        queries.deleteAllNumbers()
        update.numbers.forEach { (input, numbers) -> saveNumbers(input, numbers) }
        queries.saveDashboardToken(update.token, update.zone)
    }

    /** Removes the computer's copy while preserving phone chats and their native records. */
    fun removeComputerCopy() = queries.transaction { removeCopyOfComputer() }

    private fun removeCopyOfComputer() {
        queries.computerConversations().executeAsList().forEach(::removeConversation)
        removeComputerData()
    }

    /** Empties the conversations, the facts, the dashboard and the record of the sync. */
    fun deleteEverything() = queries.transaction { emptyCopy() }

    private fun emptyCopy() {
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
