package cf.construct.mobile.feature.template

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
 * Reads from [construct] and turns what it gets into one [TemplateState]. It is plain common
 * Kotlin with no Android type, so the Android screen and the iPhone screen watch the same class.
 *
 * A [Construct] reports a failed read as [Outcome.Failed] and does not throw, so the holder does not
 * catch. Call [refresh] and run [scope] on one thread, the main one: the newest read wins because
 * a refresh cancels the read before it, and that holds only without a second thread in between.
 *
 * @param scope where the reads run; the screen's owner cancels it.
 */
class TemplateHolder(
    private val construct: Construct,
    private val scope: CoroutineScope,
) {
    private var reading: Job? = null
    private val mutableState = MutableStateFlow(TemplateState())
    val state: StateFlow<TemplateState> = mutableState.asStateFlow()

    init {
        refresh()
    }

    /**
     * Reads again. The items stay on screen while it runs, and a read still running is dropped,
     * so the newest read is the one that shows.
     */
    fun refresh() {
        reading?.cancel()
        mutableState.update { it.copy(loading = true, failure = null) }
        reading =
            scope.launch {
                mutableState.value =
                    when (val read = construct.listConversations()) {
                        is Outcome.Ok -> TemplateState(loading = false, items = read.value.map { it.title })
                        is Outcome.Failed -> mutableState.value.copy(loading = false, failure = read.error)
                    }
            }
    }
}
