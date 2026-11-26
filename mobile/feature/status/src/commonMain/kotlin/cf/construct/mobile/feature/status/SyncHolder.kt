package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.database.SyncRecord
import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncResult
import cf.construct.mobile.core.sync.SyncStep
import cf.construct.mobile.core.sync.Syncer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds

/**
 * How far the first copy from a computer has come: the three steps in the order they are made. It exists while
 * the copy holds no whole copy from the computer it belongs to. A screen shows it as the loading screen when the
 * phone is paired; the holder does not know about pairing.
 */
data class FirstCopy(
    val steps: List<StepProgress> = SyncStep.entries.map { StepProgress(it) },
    /** Whether a sync is making the copy now. */
    val running: Boolean = false,
) {
    internal fun with(progress: StepProgress) =
        copy(steps = steps.map { if (it.step == progress.step) progress else it })
}

/** What the page tells about a sync. Only a sync the person waits for tells anything, besides "not reachable". */
enum class SyncNotice {
    None,

    /** "Syncing with your computer." */
    Syncing,

    /** "Up to date. Synced just now." for a few seconds. */
    JustSynced,

    /** "The computer is not reachable", with [SyncState.lastSyncedAt]: shown when it starts, kept till a sync works. */
    NotReachable,
}

data class SyncState(
    /** Whether the copy has been read: before that [firstCopy] and [lastSyncedAt] are not yet known. */
    val known: Boolean = false,
    val firstCopy: FirstCopy? = null,
    val notice: SyncNotice = SyncNotice.None,
    /** When the copy was last brought up to date (RFC 3339), or null if it never was. */
    val lastSyncedAt: String? = null,
)

/**
 * Runs the syncs of the app and says what a page shows about them. While the app is in the foreground it syncs
 * when it comes there and then every [interval]; nothing syncs while it is not. A sync the person waits for (the
 * one when the app opens or comes back, and [syncNow], which is "Sync now" and "Try again") shows
 * [SyncNotice.Syncing] and then [SyncNotice.JustSynced]. The check every [interval] is silent: what it copies just
 * appears on the pages. "Not reachable" shows once when it starts, and is not shown again by every check.
 *
 * Call its functions on the main thread, where [scope] runs.
 */
class SyncHolder(
    private val syncer: Syncer,
    private val scope: CoroutineScope,
    private val interval: Duration = DEFAULT_INTERVAL,
    private val justSyncedTime: Duration = JUST_SYNCED_TIME,
    private val storageDispatcher: CoroutineDispatcher = Dispatchers.Default,
) {
    /**
     * For Swift, which cannot pass a [Duration]: the period of the checks and the time "Synced just now" shows,
     * in milliseconds. [SYNC_INTERVAL_MILLIS] and [JUST_SYNCED_MILLIS] are what the apps use.
     */
    constructor(
        syncer: Syncer,
        scope: CoroutineScope,
        intervalMillis: Long,
        justSyncedMillis: Long,
    ) : this(syncer, scope, intervalMillis.milliseconds, justSyncedMillis.milliseconds)

    private val mutable = MutableStateFlow(SyncState())
    val state: StateFlow<SyncState> = mutable.asStateFlow()
    private var ticker: Job? = null
    private var running: Job? = null
    private var hiding: Job? = null
    private var userWaits = false

    init {
        scope.launch { read() }
    }

    /** True when the app is in front of the person. A change to true is a sync the person waits for. */
    fun setForeground(active: Boolean) {
        if (active == (ticker != null)) return
        ticker?.cancel()
        ticker = null
        if (active) {
            ticker =
                scope.launch {
                    request(waits = true)
                    while (true) {
                        delay(interval)
                        request(waits = false)
                    }
                }
        } else {
            running?.cancel()
            mutable.update {
                it.copy(
                    firstCopy = it.firstCopy?.copy(running = false),
                    notice = if (it.notice == SyncNotice.Syncing) SyncNotice.None else it.notice,
                )
            }
        }
    }

    /** "Sync now" and "Try again": a sync the person waits for. */
    fun syncNow() = request(waits = true)

    /**
     * A check like the one every [interval], now. The app asks for it after the phone is unpaired: the check finds
     * no computer to miss, so a "not reachable" of the old computer goes now and not at the next check.
     */
    fun checkNow() = request(waits = false)

    /**
     * What the copy remembers, or null when the copy cannot be read (a full disk, a damaged file). Then the state
     * stays as it was: it is not guessed, and the next check reads again.
     */
    @Suppress("TooGenericExceptionCaught")
    private suspend fun readRecord(): SyncRecord? =
        try {
            withContext(storageDispatcher) { syncer.record() }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            null
        }

    private suspend fun read() {
        val record = readRecord() ?: return
        mutable.update {
            it.copy(
                known = true,
                firstCopy = if (record.firstCopyDone) null else it.firstCopy ?: FirstCopy(),
                lastSyncedAt = record.syncedAt,
            )
        }
    }

    private fun request(waits: Boolean) {
        if (running?.isActive == true) {
            if (waits && !userWaits) {
                userWaits = true
                if (mutable.value.firstCopy == null) mutable.update { it.copy(notice = SyncNotice.Syncing) }
            }
            return
        }
        userWaits = waits
        running = scope.launch { run() }
    }

    private suspend fun run() {
        hiding?.cancel()
        read()
        mutable.update { current ->
            if (current.firstCopy != null) {
                current.copy(firstCopy = current.firstCopy.copy(running = true))
            } else if (userWaits) {
                current.copy(notice = SyncNotice.Syncing)
            } else {
                current
            }
        }
        val firstCopy = mutable.value.firstCopy != null
        val result = syncer.sync { progress -> mutable.update { it.copy(firstCopy = it.firstCopy?.with(progress)) } }
        // A sync that was cancelled never gets here: the cancel moves on.
        finish(result, firstCopy)
    }

    /** The first copy has its own screen, so it is not followed by "Synced just now". */
    private suspend fun finish(
        result: SyncResult,
        firstCopy: Boolean,
    ) {
        when (result) {
            SyncResult.Done -> {
                val record = readRecord()
                val notice = if (userWaits && !firstCopy) SyncNotice.JustSynced else SyncNotice.None
                mutable.update {
                    it.copy(firstCopy = null, notice = notice, lastSyncedAt = record?.syncedAt ?: it.lastSyncedAt)
                }
                if (notice == SyncNotice.JustSynced) hideJustSyncedLater()
            }

            is SyncResult.Missed -> {
                mutable.update {
                    it.copy(
                        firstCopy = it.firstCopy?.copy(running = false),
                        notice = noticeAfterMiss(it, result.link),
                    )
                }
            }

            SyncResult.Cut -> {
                mutable.update {
                    val notice = if (it.notice == SyncNotice.Syncing) SyncNotice.None else it.notice
                    it.copy(firstCopy = it.firstCopy?.copy(running = false), notice = notice)
                }
            }
        }
    }

    /**
     * "Not reachable" shows when a sync misses the computer, and stays until a sync works, so the checks that follow
     * do not show it again. A phone that is no longer paired has no computer to miss, so it shows nothing. A miss for
     * another reason ends "syncing" and shows nothing: the Computer page says why.
     */
    private fun noticeAfterMiss(
        current: SyncState,
        link: ComputerLink,
    ): SyncNotice =
        when {
            link == ComputerLink.Unreachable || link == ComputerLink.NoNetwork -> SyncNotice.NotReachable
            link == ComputerLink.NotPaired || current.notice == SyncNotice.Syncing -> SyncNotice.None
            else -> current.notice
        }

    private fun hideJustSyncedLater() {
        hiding =
            scope.launch {
                delay(justSyncedTime)
                mutable.update { if (it.notice == SyncNotice.JustSynced) it.copy(notice = SyncNotice.None) else it }
            }
    }

    private companion object {
        val DEFAULT_INTERVAL = SYNC_INTERVAL_MILLIS.milliseconds
        val JUST_SYNCED_TIME = JUST_SYNCED_MILLIS.milliseconds
    }
}

/** How often the app checks the computer while it is in front of the person. */
const val SYNC_INTERVAL_MILLIS = 60_000L

/** How long "Up to date. Synced just now." shows. */
const val JUST_SYNCED_MILLIS = 3_000L
