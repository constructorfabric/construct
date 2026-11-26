package cf.construct.mobile.android

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import cf.construct.mobile.core.sync.Syncer
import cf.construct.mobile.feature.status.SyncHolder
import kotlin.time.Duration.Companion.milliseconds

/** Keeps the sync holder across a turn of the screen, so a sync that is under way is not lost to it. */
internal class SyncViewModel(
    syncer: Syncer,
) : ViewModel() {
    val holder = SyncHolder(syncer, viewModelScope, SyncTesting.intervalMillis.milliseconds)
}
