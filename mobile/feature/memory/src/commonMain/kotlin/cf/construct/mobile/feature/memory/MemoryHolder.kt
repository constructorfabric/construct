package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Watches the facts of [construct] and turns what it gets into one [MemoryState]. It is plain common
 * Kotlin with no Android type, so the Android screen and the iPhone screen watch the same class.
 *
 * The facts are a stream, so a change in the copy shows by itself. A [Construct] reports a failed read
 * as [Outcome.Failed] and goes on, so the holder does not catch: the failure shows beside the facts
 * already on screen, and the next good read clears it. Call [refresh] and run [scope] on one thread,
 * the main one: the newest watch wins because a refresh cancels the watch before it, and that holds
 * only without a second thread in between.
 *
 * @param scope where the watch runs; the screen's owner cancels it.
 */
class MemoryHolder(
    private val construct: Construct,
    private val scope: CoroutineScope,
) {
    private var watching: Job? = null
    private val mutableState = MutableStateFlow(MemoryState())
    val state: StateFlow<MemoryState> = mutableState.asStateFlow()

    init {
        refresh()
    }

    /**
     * Starts the watch again, to read at once. The facts stay on screen while it runs, and a watch
     * still running is dropped, so the newest watch is the one that shows.
     */
    fun refresh() {
        watching?.cancel()
        mutableState.update { it.copy(loading = true, failure = null) }
        watching =
            scope.launch {
                construct.memoryFacts.collect { read ->
                    mutableState.value =
                        when (read) {
                            is Outcome.Ok -> MemoryState(loading = false, facts = read.value.facts)
                            is Outcome.Failed -> mutableState.value.copy(loading = false, failure = read.error)
                        }
                }
            }
    }
}
