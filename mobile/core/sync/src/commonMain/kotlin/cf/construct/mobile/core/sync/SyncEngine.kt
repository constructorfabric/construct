package cf.construct.mobile.core.sync

import cf.construct.mobile.core.connection.ComputerAccess
import cf.construct.mobile.core.connection.ComputerChannel
import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.ComputerReach
import cf.construct.mobile.core.database.ChatOnCopy
import cf.construct.mobile.core.database.ChatUpdate
import cf.construct.mobile.core.database.ChatsUpdate
import cf.construct.mobile.core.database.CopyChanged
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.CopyUpdate
import cf.construct.mobile.core.database.DashboardUpdate
import cf.construct.mobile.core.database.FactsUpdate
import cf.construct.mobile.core.database.SyncRecord
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.time.Clock

/** The three steps of a copy, in the order they are made. */
enum class SyncStep { MEMORY, CHATS, DASHBOARD }

/**
 * How far one step has come: how many items it has copied, and how many there are once the step knows (the
 * facts and the numbers are counted as their pages come, so their total is known only when they end).
 */
data class StepProgress(
    val step: SyncStep,
    val copied: Int = 0,
    val total: Int? = null,
    val done: Boolean = false,
)

/** How one run of a sync ended. */
sealed interface SyncResult {
    /** The copy is as the computer is now. Nothing was written to it when nothing had changed. */
    data object Done : SyncResult

    /** The computer was not reached, for the reason of [link]. The copy is as it was. */
    data class Missed(
        val link: ComputerLink,
    ) : SyncResult

    /** The computer answered but not what a sync needs (it is erasing), or the copy moved on. The copy is as it was. */
    data object Cut : SyncResult
}

/** What the sync holder asks of a sync: to run one, and to say how the copy stands. */
interface Syncer {
    /**
     * Brings the copy up to date with the computer, whole or not at all. [progress] says how far each step has
     * come while the first copy is made, and is called from another thread than the caller's.
     */
    suspend fun sync(progress: (StepProgress) -> Unit = {}): SyncResult

    /** What the copy remembers: the computer, the time of the last sync, and whether a first copy is whole. */
    fun record(): SyncRecord
}

/** The most times a part is read again when it keeps changing under the reader. */
private const val MAX_READS = 3

/**
 * Copies what the paired computer keeps into the copy on the phone: its facts, its chats and its dashboard.
 *
 * One run asks the computer which parts changed (a token for each, kept from the last copy), reads whole the
 * parts that did, and then applies everything in one transaction of the copy ([CopyStore.applySync]), so a reader
 * of the copy sees it as it was or as it is, and a sync that is cut (the computer stops, the network is lost, the
 * caller is cancelled) leaves it as it was. Nothing is sent to the computer but the read commands, and no text of
 * a fact or a chat and no number is put into a log or an error.
 *
 * It also holds the rules of the two things that empty a copy: a computer that erased everything about the person
 * (its reset marker differs from the one the copy kept: the copy is emptied, the chats started on the phone too,
 * and refilled), and another computer than the one the copy came from (the copy of the old computer's data goes).
 */
class SyncEngine(
    private val computer: ComputerAccess,
    private val copy: CopyStore,
    private val dispatcher: CoroutineDispatcher = Dispatchers.Default,
    private val now: () -> String = { Clock.System.now().toString() },
) : Syncer {
    private val running = Mutex()

    override fun record(): SyncRecord = copy.syncRecord()

    override suspend fun sync(progress: (StepProgress) -> Unit): SyncResult =
        running.withLock {
            when (val reached = computer.reach { fetch(it, progress) }) {
                is ComputerReach.Missed -> SyncResult.Missed(reached.link)
                is ComputerReach.Reached -> reached.value?.let { apply(it) } ?: SyncResult.Cut
            }
        }

    /** What one run read from the computer, ready to be applied to the copy in one transaction. */
    private class Fetched(
        val update: CopyUpdate,
    )

    private fun fetch(
        channel: ComputerChannel,
        progress: (StepProgress) -> Unit,
    ): Fetched? =
        try {
            val identity = channel.identity
            var record = stored { copy.syncRecord() }
            // The pairing already did this. A copy that still belongs to another computer (a pairing that was cut
            // between the two stores) is emptied here, before anything of this computer is read into it.
            if (record.computer != identity) {
                stored { copy.adoptComputer(identity) }
                record = stored { copy.syncRecord() }
            }
            val check = channel.check()
            val marker = check.reset
            // A copy that has not seen a marker takes the computer's current one and empties nothing: only a marker
            // that differs from one the copy saw means that the computer erased everything.
            if (marker != null && record.resetMarker != null && record.resetMarker != marker) {
                stored { copy.reset(marker) }
                record = stored { copy.syncRecord() }
            }
            Fetched(
                CopyUpdate(
                    computer = identity,
                    syncedAt = now(),
                    resetMarker = marker,
                    facts = changed(check.facts, record.factsToken) { facts(channel, progress) },
                    chats = changed(check.chats, record.chatsToken) { chats(channel, progress) },
                    dashboard = changed(check.dashboard, record.dashboardToken) { dashboard(channel, progress) },
                ),
            )
        } catch (_: Cut) {
            null
        }

    /**
     * A call to the copy. A copy that cannot be read or written (a full disk, a damaged file) is not the computer's
     * fault and no reason to stop the checks: the sync is cut, the copy is as it was, and the next check tries
     * again. What the store says is dropped, because it can name what the copy holds.
     */
    @Suppress("TooGenericExceptionCaught")
    private inline fun <T> stored(call: () -> T): T =
        try {
            call()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            throw Cut()
        }

    /**
     * Reads a part when the computer can give it (it has a token) and the copy does not hold that token. A part the
     * computer cannot give now is left as it is, and read at a later sync: the others are not held back by it.
     */
    private fun <T> changed(
        now: String?,
        kept: String?,
        read: () -> T,
    ): T? = if (now == null || now == kept) null else readAgainWhenPartsMoved(read)

    private fun <T> readAgainWhenPartsMoved(read: () -> T): T? {
        repeat(MAX_READS) {
            try {
                return read()
            } catch (_: PartChanged) {
                // The pages did not fit together: read it again.
            } catch (_: PartUnavailable) {
                return null
            }
        }
        throw Cut()
    }

    private fun facts(
        channel: ComputerChannel,
        progress: (StepProgress) -> Unit,
    ): FactsUpdate {
        progress(StepProgress(SyncStep.MEMORY))
        val read = channel.pages("sync-facts") { progress(StepProgress(SyncStep.MEMORY, copied = it)) }
        val facts = read.items.mapNotNull(::fact)
        progress(StepProgress(SyncStep.MEMORY, facts.size, facts.size, done = true))
        return FactsUpdate(read.token, facts)
    }

    private fun chats(
        channel: ComputerChannel,
        progress: (StepProgress) -> Unit,
    ): ChatsUpdate {
        progress(StepProgress(SyncStep.CHATS))
        val held = stored { copy.computerChats() }.associateBy(ChatOnCopy::id)
        val list = channel.pages("sync-chats") { progress(StepProgress(SyncStep.CHATS, copied = it)) }
        val headers = list.items.mapNotNull(::chatHeader)
        val chats =
            headers.mapIndexed { index, header ->
                progress(StepProgress(SyncStep.CHATS, copied = index, total = headers.size))
                chat(channel, header, held[header.id], list.token)
            }
        progress(StepProgress(SyncStep.CHATS, headers.size, headers.size, done = true))
        return ChatsUpdate(list.token, chats)
    }

    /**
     * A chat that grew is read from the line the copy stops at. A chat the copy holds more lines of than the
     * computer counts is read whole and replaces it, and a chat that has the lines it should is not read at all.
     */
    private fun chat(
        channel: ComputerChannel,
        header: ChatHeader,
        held: ChatOnCopy?,
        token: String,
    ): ChatUpdate {
        val from = held?.items?.takeIf { it <= header.count } ?: 0
        val lines =
            if (from < header.count) {
                channel.pages("sync-chat", id = header.id, from = from, token = token).items.map(::chatLine)
            } else {
                emptyList()
            }
        val time = lines.lastOrNull()?.time ?: header.started.takeIf { from == 0 }
        return ChatUpdate(header.id, header.title, time, from, lines.map { it.message })
    }

    private fun dashboard(
        channel: ComputerChannel,
        progress: (StepProgress) -> Unit,
    ): DashboardUpdate {
        progress(StepProgress(SyncStep.DASHBOARD))
        val document = channel.document()
        val read =
            channel.pages("sync-dashboard", token = document.token) {
                progress(StepProgress(SyncStep.DASHBOARD, copied = it))
            }
        val numbers = withSessions(read.items.mapNotNull(::dashboardNumbers), sessions(channel, read.token))
        progress(StepProgress(SyncStep.DASHBOARD, numbers.size, numbers.size, done = true))
        return DashboardUpdate(read.token, document.zone, document.document?.let(::dashboardDocument), numbers)
    }

    /**
     * The sessions behind the groups, which a computer that does not know them refuses: the numbers are then kept
     * without them. A part that changed meanwhile is read again, as the numbers are.
     */
    private fun sessions(
        channel: ComputerChannel,
        token: String,
    ): List<SessionsItem> =
        try {
            channel.pages("sync-dashboard-sessions", token = token).items.mapNotNull(::sessionsItem)
        } catch (_: PartUnavailable) {
            emptyList()
        }

    /** One transaction of the copy. A copy that moved on, or cannot be written, is a sync that is cut. */
    @Suppress("TooGenericExceptionCaught")
    private suspend fun apply(fetched: Fetched): SyncResult =
        withContext(dispatcher) {
            try {
                copy.applySync(fetched.update)
                SyncResult.Done
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: CopyChanged) {
                SyncResult.Cut
            } catch (_: Exception) {
                SyncResult.Cut
            }
        }
}
