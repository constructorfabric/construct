package cf.construct.mobile.android

import cf.construct.mobile.core.sync.Syncer
import cf.construct.mobile.feature.status.SYNC_INTERVAL_MILLIS

/** How often the app checks the computer. Only debug builds let a test change it. */
internal object SyncTesting {
    val intervalMillis: Long get() = SYNC_INTERVAL_MILLIS

    fun wrap(base: Syncer): Syncer = base
}
