package cf.construct.mobile.android

import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncResult
import cf.construct.mobile.core.sync.Syncer
import cf.construct.mobile.feature.status.SYNC_INTERVAL_MILLIS
import kotlinx.coroutines.delay

/**
 * What a test changes about the sync, in a debug build only: how often the app checks the computer, and a pause
 * ([pauseMillis]) before each sync (three times that) and after each report of progress, so a test that looks every
 * second or so can see "Syncing" and the counts of the first copy rise. A test sets them before it starts the app's
 * screen; the pause is read at each sync, and the test sets both back when it ends.
 */
internal object SyncTesting {
    @Volatile
    var intervalMillis: Long = SYNC_INTERVAL_MILLIS

    @Volatile
    var pauseMillis: Long = 0

    fun wrap(base: Syncer): Syncer = PacedSyncer(base)
}

private class PacedSyncer(
    private val base: Syncer,
) : Syncer {
    override suspend fun sync(progress: (StepProgress) -> Unit): SyncResult {
        val pause = SyncTesting.pauseMillis
        if (pause > 0) delay(pause * BEFORE_EACH_SYNC)
        return base.sync { report ->
            progress(report)
            if (pause > 0) hold(pause)
        }
    }

    override fun record() = base.record()

    // The report comes from the thread of the sync, which waits here and not in a coroutine.
    private fun hold(millis: Long) = Thread.sleep(millis)

    private companion object {
        const val BEFORE_EACH_SYNC = 3
    }
}
