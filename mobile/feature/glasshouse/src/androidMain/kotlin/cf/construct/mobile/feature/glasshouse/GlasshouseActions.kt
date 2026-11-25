package cf.construct.mobile.feature.glasshouse

/** What the person does on the Glasshouse page. The screen passes these on and decides nothing. */
class GlasshouseActions(
    /** The person typed an address and asked to go; true when it opens, false when it was refused. */
    val onGo: (String) -> Boolean,
    /** The person edited the address they were typing. */
    val onEdited: () -> Unit,
    val onReload: () -> Unit,
    val onBack: () -> Unit,
)
