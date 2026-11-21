package cf.construct.mobile.feature.template

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Reads from [construct] and turns what it gets into one [TemplateState]. It is plain common
 * Kotlin with no Android type, so the Android screen and the iPhone screen watch the same class.
 *
 * @param scope where the reads run; the screen's owner cancels it.
 */
class TemplateHolder(
    private val construct: Construct,
    private val scope: CoroutineScope,
) {
    private val mutableState = MutableStateFlow(TemplateState())
    val state: StateFlow<TemplateState> = mutableState.asStateFlow()

    init {
        refresh()
    }

    /** Reads again. The items stay on screen while it runs. */
    fun refresh() {
        mutableState.update { it.copy(loading = true) }
        scope.launch {
            val next =
                when (val read = construct.listConversations()) {
                    is Outcome.Ok -> TemplateState(loading = false, items = read.value.map { it.title })
                    is Outcome.Failed -> mutableState.value.copy(loading = false, failure = read.error)
                }
            mutableState.value = next
        }
    }
}
