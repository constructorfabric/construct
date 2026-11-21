package cf.construct.mobile.feature.template

/** Everything the screen shows, in one immutable value. */
data class TemplateState(
    val loading: Boolean = true,
    val items: List<String> = emptyList(),
    /** Why the last load failed, or null. */
    val failure: String? = null,
)
