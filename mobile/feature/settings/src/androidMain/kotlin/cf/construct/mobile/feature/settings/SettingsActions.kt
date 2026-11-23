package cf.construct.mobile.feature.settings

/** What the person does on the model setup. The screen passes these on and decides nothing. */
class SettingsActions(
    val onChange: (SettingsField, String) -> Unit,
    val onSave: () -> Unit,
    val onTest: () -> Unit,
    val onRemoveKey: () -> Unit,
    val onTryAgain: () -> Unit,
)
