package cf.construct.mobile.core.construct

import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.EraseStarted
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.flow.Flow

/** Why `getConversation` fails for an id the copy does not hold. */
const val NO_SUCH_CONVERSATION = "no such conversation"

/** Why a chat cannot start until a model is configured. */
const val NO_CHAT_MODEL = "no model is set"

interface ChatApi {
    /** The conversations, the newest first. */
    suspend fun listConversations(): Outcome<List<ConversationSummary>>

    suspend fun getConversation(conversation: String): Outcome<List<ChatMessage>>

    /** One conversation, or [cf.construct.mobile.core.model.DELETE_ALL_CONVERSATIONS]. */
    suspend fun deleteConversation(which: String): Outcome<Unit>

    /** An accepted send ends through [chatEvents], even if it fails before Start. A rejected send emits no events. */
    suspend fun sendMessage(input: ChatInput): Outcome<Unit>

    /** Asks for the running answer to end; the end comes as an event. */
    suspend fun stopAnswer()

    /** The events of the answers; collect it before sending to see every event. */
    val chatEvents: Flow<ChatEvent>

    /**
     * A signal, not the conversations: it comes when it is collected and after each change to the list or to
     * the lines of any conversation, whoever made it (a sync, an answer, a delete). A page that shows the list
     * reads it again on each signal. It never ends.
     */
    val conversationChanges: Flow<Unit>
}

interface MemoryApi {
    /**
     * Every fact of the person, with the profile's version: what the copy holds now, and again each
     * time it changes. It never ends and never throws. A read that fails is an [Outcome.Failed] item
     * and the stream goes on, so a collector gets the facts again once they can be read.
     */
    val memoryFacts: Flow<Outcome<MemoryFacts>>

    /** Only the profile's version. */
    suspend fun getMemoryVersion(): Outcome<Long?>

    /**
     * Deletes one fact by its id. Sending it again after a failure is safe. The phone's copy refuses it: a fact
     * belongs to the computer, and nothing the person does on the phone changes the computer.
     */
    suspend fun deleteMemoryFact(factId: String): Outcome<Unit>

    /** Asks for everything about the person to be erased; it does not wait for the end. */
    suspend fun eraseEverything(): Outcome<EraseStarted>

    /** Whether an erasure is still running. */
    suspend fun getErasing(): Outcome<Boolean>
}

interface DashboardApi {
    suspend fun getDashboardNumbers(input: DashboardInput): Outcome<DashboardNumbers>

    /** The last good document, or none, and the problem of the file as it is now. */
    suspend fun getDashboardDocument(): Outcome<DocumentState>

    /**
     * A signal, not the dashboard: it comes when it is collected and after each change to the document or to
     * the numbers. A page that shows them reads them again on each signal. It never ends.
     */
    val dashboardChanges: Flow<Unit>
}

/** The one interface the screens use for chats, memory and the dashboard. */
interface Construct :
    ChatApi,
    MemoryApi,
    DashboardApi
