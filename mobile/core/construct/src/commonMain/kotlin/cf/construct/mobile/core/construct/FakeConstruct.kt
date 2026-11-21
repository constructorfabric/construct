package cf.construct.mobile.core.construct

import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.GroupBy
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.MemoryFact
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Position
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.Widget
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonPrimitive

/**
 * A [Construct] with seeded data and a canned streamed answer, for tests and previews. It keeps
 * its data in memory and talks to nothing.
 *
 * @param scope where the canned answer runs.
 * @param deltaDelayMillis the pause between the pieces of the answer.
 */
class FakeConstruct(
    private val scope: CoroutineScope,
    private val deltaDelayMillis: Long = 0,
    conversations: Map<ConversationSummary, List<ChatMessage>> = seededConversations,
    facts: List<MemoryFact> = seededFacts,
    private val document: DashboardDocument? = seededDocument,
) : Construct {
    private val threads = LinkedHashMap(conversations)
    private val facts = facts.toMutableList()
    private val events = MutableSharedFlow<ChatEvent>(extraBufferCapacity = EVENT_BUFFER)
    private var answer: Job? = null
    private var erased = false

    override val chatEvents: Flow<ChatEvent> = events.asSharedFlow()

    override suspend fun listConversations(): Outcome<List<ConversationSummary>> =
        Outcome.Ok(threads.keys.sortedByDescending { it.time })

    override suspend fun getConversation(conversation: String): Outcome<List<ChatMessage>> {
        val thread = threads.entries.firstOrNull { it.key.id == conversation }?.value
        return if (thread == null) Outcome.Failed(NO_SUCH_CONVERSATION) else Outcome.Ok(thread)
    }

    override suspend fun deleteConversation(which: String): Outcome<Unit> {
        if (which == DELETE_ALL_CONVERSATIONS) {
            threads.clear()
        } else {
            threads.keys.removeAll { it.id == which }
        }
        return Outcome.Ok(Unit)
    }

    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> {
        if (answer?.isActive == true) return Outcome.Failed(ANSWER_RUNNING)
        val id = input.conversation.ifEmpty { "new-${threads.size + 1}" }
        answer = scope.launch { streamAnswer(id, input) }
        return Outcome.Ok(Unit)
    }

    override suspend fun stopAnswer() {
        answer?.cancel()
    }

    private suspend fun streamAnswer(
        id: String,
        input: ChatInput,
    ) {
        events.emit(ChatEvent.Start(id))
        events.emit(ChatEvent.Tool(CANNED_TOOL_LINE))
        var stopped = false
        try {
            for (piece in CANNED_ANSWER) {
                delay(deltaDelayMillis)
                events.emit(ChatEvent.Delta(piece))
            }
        } catch (_: CancellationException) {
            stopped = true
        }
        record(id, input, stopped)
        // The answer may have been cancelled; the end still goes out.
        withContext(NonCancellable) { events.emit(ChatEvent.Done(stopped)) }
    }

    private fun record(
        id: String,
        input: ChatInput,
        stopped: Boolean,
    ) {
        val known = threads.keys.firstOrNull { it.id == id }
        val summary = known ?: ConversationSummary(id, input.message.take(TITLE_LENGTH), NEW_CHAT_TIME)
        val earlier = threads[summary].orEmpty()
        val reply =
            if (stopped) {
                emptyList()
            } else {
                listOf(
                    ChatMessage(ChatRole.ASSISTANT, CANNED_ANSWER.joinToString("")),
                )
            }
        threads[summary] = earlier + ChatMessage(ChatRole.USER, input.message) + reply
    }

    override suspend fun getMemoryFacts(): Outcome<MemoryFacts> =
        Outcome.Ok(MemoryFacts(facts.toList(), if (erased) null else FACTS_VERSION))

    override suspend fun getMemoryVersion(): Outcome<Long?> = Outcome.Ok(if (erased) null else FACTS_VERSION)

    override suspend fun deleteMemoryFact(factId: String): Outcome<Unit> {
        facts.removeAll { it.factId == factId }
        return Outcome.Ok(Unit)
    }

    override suspend fun eraseEverything(): Outcome<EraseStarted> {
        val already = erased
        facts.clear()
        threads.clear()
        erased = true
        return Outcome.Ok(EraseStarted(running = already))
    }

    override suspend fun getErasing(): Outcome<Boolean> = Outcome.Ok(false)

    override suspend fun getDashboardNumbers(input: DashboardInput): Outcome<DashboardNumbers> {
        val days = input.days.mapIndexed { index, day -> DayMinutes(day.label, MINUTES_PER_DAY * (index % 3 + 1)) }
        val total = days.sumOf { it.minutes }
        val groups =
            GROUP_KEYS.mapIndexed { index, key ->
                val minutes = total * GROUP_PERCENT[index] / PERCENT
                GroupMinutes(key, minutes, GROUP_PERCENT[index] / PERCENT.toDouble(), sessions = index + 1)
            }
        return Outcome.Ok(DashboardNumbers(total, sessionCount = days.size * 2, days = days, groups = groups))
    }

    override suspend fun getDashboardDocument(): Outcome<DocumentState> = Outcome.Ok(DocumentState(document, null))

    companion object {
        const val NO_SUCH_CONVERSATION = "no such conversation"
        const val ANSWER_RUNNING = "an answer is running"
        const val CANNED_TOOL_LINE = "Looked up your profile"
        val CANNED_ANSWER = listOf("Here is ", "a canned ", "answer.")

        private const val EVENT_BUFFER = 64
        private const val TITLE_LENGTH = 60
        private const val NEW_CHAT_TIME = "2026-01-01T00:00:00Z"
        private const val FACTS_VERSION = 3L
        private const val MINUTES_PER_DAY = 60
        private const val PERCENT = 100
        private val GROUP_KEYS = listOf("alpha", "beta")
        private val GROUP_PERCENT = listOf(70, 30)

        val seededConversations: Map<ConversationSummary, List<ChatMessage>> =
            mapOf(
                ConversationSummary("c1", "What do you know about me?", "2026-01-02T09:00:00Z") to
                    listOf(
                        ChatMessage(ChatRole.USER, "What do you know about me?"),
                        ChatMessage(ChatRole.TOOL, "Looked up your profile"),
                        ChatMessage(ChatRole.ASSISTANT, "You are a mobile developer who prefers short answers."),
                    ),
                ConversationSummary("c2", "Plan my week", "2026-01-01T18:30:00Z") to
                    listOf(
                        ChatMessage(ChatRole.USER, "Plan my week"),
                        ChatMessage(ChatRole.ASSISTANT, "Start with the release notes on Monday."),
                    ),
            )

        val seededFacts: List<MemoryFact> =
            listOf(
                MemoryFact("f1", EntityKind.IDENTITY, "name", JsonPrimitive("Alex Example"), "2026-01-01T08:00:00Z"),
                MemoryFact("f2", EntityKind.ROLE, "title", JsonPrimitive("Mobile developer"), "2026-01-01T08:01:00Z"),
                MemoryFact("f3", EntityKind.SKILL, "skills", JsonPrimitive("Kotlin"), "2026-01-01T08:02:00Z", "editor"),
                MemoryFact(
                    "f4",
                    EntityKind.PREFERENCE,
                    "style",
                    JsonPrimitive("Short answers"),
                    "2026-01-01T08:03:00Z",
                ),
            )

        val seededDocument =
            DashboardDocument(
                title = "My week",
                period = Period.THIS_WEEK,
                sessions = Whose.ALL,
                widgets =
                    listOf(
                        Widget("total", WidgetKind.TOTAL, Position(0, 0, 4, 2)),
                        Widget("days", WidgetKind.BARS_PER_DAY, Position(4, 0, 8, 2)),
                        Widget("by-project", WidgetKind.TABLE, Position(0, 2, 12, 4), groupBy = GroupBy.PROJECT),
                    ),
            )
    }
}
