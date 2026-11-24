package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.NO_SUCH_CONVERSATION
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
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withContext

/**
 * A [Construct] that answers the reads and the deletes from the phone's copy. It starts no answer:
 * sending fails and stopping does nothing, and no event ever comes.
 *
 * @param dispatcher where the database is read and written.
 */
class DatabaseConstruct(
    private val store: CopyStore,
    private val dispatcher: CoroutineDispatcher = Dispatchers.Default,
) : Construct {
    override val chatEvents: Flow<ChatEvent> = emptyFlow()

    override suspend fun listConversations(): Outcome<List<ConversationSummary>> =
        onDisk { Outcome.Ok(store.conversations()) }

    override suspend fun getConversation(conversation: String): Outcome<List<ChatMessage>> =
        onDisk {
            store.messages(conversation)?.let { Outcome.Ok(it) } ?: Outcome.Failed(NO_SUCH_CONVERSATION)
        }

    override suspend fun deleteConversation(which: String): Outcome<Unit> =
        onDisk {
            store.deleteConversation(which)
            Outcome.Ok(Unit)
        }

    override suspend fun sendMessage(input: ChatInput): Outcome<Unit> = Outcome.Failed(NOT_AVAILABLE_YET)

    override suspend fun stopAnswer() = Unit

    /**
     * The facts now, and again after each write to the copy that changes them or the version. A write
     * that leaves what is read as it was gives nothing, so a sync that finds nothing new is silent.
     */
    override val memoryFacts: Flow<Outcome<MemoryFacts>> =
        store
            .factChanges()
            .conflate()
            .map { onDisk { Outcome.Ok(store.facts()) } }
            .distinctUntilChanged()

    override suspend fun getMemoryVersion(): Outcome<Long?> = onDisk { Outcome.Ok(store.memoryVersion()) }

    override suspend fun deleteMemoryFact(factId: String): Outcome<Unit> =
        onDisk {
            store.deleteFact(factId)
            Outcome.Ok(Unit)
        }

    /** Empties the copy; the copy cannot tell whether an erasure was already on, so it says it was not. */
    override suspend fun eraseEverything(): Outcome<EraseStarted> =
        onDisk {
            store.deleteEverything()
            Outcome.Ok(EraseStarted(running = false))
        }

    override suspend fun getErasing(): Outcome<Boolean> = Outcome.Ok(false)

    override suspend fun getDashboardNumbers(input: DashboardInput): Outcome<DashboardNumbers> =
        onDisk { store.numbers(input)?.let { Outcome.Ok(it) } ?: Outcome.Failed(NO_NUMBERS) }

    override suspend fun getDashboardDocument(): Outcome<DocumentState> = onDisk { Outcome.Ok(store.document()) }

    /** A copy that cannot be read or written (a damaged row, a full disk) is a failed call, not a crash. */
    @Suppress("TooGenericExceptionCaught")
    private suspend fun <T> onDisk(block: () -> Outcome<T>): Outcome<T> =
        withContext(dispatcher) {
            try {
                block()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                Outcome.Failed(UNREADABLE)
            }
        }

    companion object {
        const val UNREADABLE = "the copy could not be read"
        const val NOT_AVAILABLE_YET = "not available yet"
        const val NO_NUMBERS = "no numbers for these days"
    }
}
