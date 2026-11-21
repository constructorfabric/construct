package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.model.MemoryFact

/** Everything the Memory screen shows, in one immutable value. */
data class MemoryState(
    val loading: Boolean = true,
    val facts: List<MemoryFact> = emptyList(),
    /** Why the last load failed, or null. */
    val failure: String? = null,
)
