package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Reads from [construct] and turns what it gets into one [MemoryState]. It is plain common
 * Kotlin with no Android type, so the Android screen and the iPhone screen watch the same class.
 *
 * @param scope where the reads run; the screen's owner cancels it.
 */
class MemoryHolder(
    private val construct: Construct,
    private val scope: CoroutineScope,
) {
    private var reading: Job? = null
    private val mutableState = MutableStateFlow(MemoryState())
    val state: StateFlow<MemoryState> = mutableState.asStateFlow()

    init {
        refresh()
    }

    /**
     * Reads again. The facts stay on screen while it runs, and a read still running is dropped,
     * so the newest read is the one that shows.
     */
    fun refresh() {
        reading?.cancel()
        mutableState.update { it.copy(loading = true, failure = null) }
        reading =
            scope.launch {
                mutableState.value =
                    when (val read = readOrFail()) {
                        is Outcome.Ok -> MemoryState(loading = false, facts = read.value.facts)
                        is Outcome.Failed -> mutableState.value.copy(loading = false, failure = read.error)
                    }
            }
    }

    /** An implementation that throws becomes a failed read, so one bad read cannot cancel the scope. */
    @Suppress("TooGenericExceptionCaught")
    private suspend fun readOrFail() =
        try {
            construct.getMemoryFacts()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            Outcome.Failed(failure.message ?: failure::class.simpleName.orEmpty())
        }
}
