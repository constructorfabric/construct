package cf.construct.mobile.feature.settings

/** The three fields of the form. */
enum class SettingsField { Address, ModelName, Key }

/** How one line of the model check ended. */
enum class CheckStatus { Checking, Works, Failed, Skipped }

/** One line of the model check: "Answers" or "Uses tools", and why it failed when it did. */
data class CheckLine(
    val status: CheckStatus,
    val reason: String? = null,
)

/** The model check as the screen shows it; [None] before the first Test. */
sealed interface SettingsCheck {
    data object None : SettingsCheck

    data class Shown(
        val answers: CheckLine,
        val tools: CheckLine,
    ) : SettingsCheck
}

/**
 * Everything the model setup shows, in one immutable value. It holds no key: [keyEntry] is only what
 * the person is typing now, and a saved key shows as [keyIsSet].
 */
data class SettingsState(
    val loading: Boolean = true,
    /** Why the stored setting could not be read, or null. */
    val unavailable: String? = null,
    val address: String = "",
    val model: String = "",
    val keyEntry: String = "",
    val keyIsSet: Boolean = false,
    val addressMissing: Boolean = false,
    val modelMissing: Boolean = false,
    val saved: Boolean = false,
    val testing: Boolean = false,
    val check: SettingsCheck = SettingsCheck.None,
    /** Why the last Save, Test or removal could not be done, or null. */
    val failure: String? = null,
    internal val pendingWrites: Int = 0,
) {
    val canTest: Boolean get() = !testing && pendingWrites == 0 && address.isNotBlank() && model.isNotBlank()

    override fun toString(): String =
        "SettingsState(address=$address, model=$model, keyIsSet=$keyIsSet, keyTyped=${keyEntry.isNotEmpty()})"
}
