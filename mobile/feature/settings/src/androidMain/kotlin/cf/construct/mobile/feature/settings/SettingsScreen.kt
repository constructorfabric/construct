package cf.construct.mobile.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

private const val REQUIRED = "This field is required"
private const val KEY_SAVED_HINT = "A key is saved. Leave this empty to keep it."
private const val KEY_DOTS = "••••••••"

/** Shows [state] and loads nothing: the model setup, its states and its notices. */
@Composable
fun SettingsScreen(
    state: SettingsState,
    actions: SettingsActions,
    modifier: Modifier = Modifier,
) {
    when {
        state.unavailable != null -> {
            Column(modifier.fillMaxSize().padding(Spacing.L.dp)) {
                MessageLine(
                    MessageKind.Error,
                    "The settings are not available: ${state.unavailable}",
                    Modifier.testTag("settings-unavailable"),
                )
                TextButton(onClick = actions.onTryAgain) { Text("Try again", fontWeight = FontWeight.Bold) }
            }
        }

        state.loading -> {
            Column(
                modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(Spacing.M.dp, Alignment.CenterVertically),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                CircularProgressIndicator()
                Text("Loading the settings", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }

        else -> {
            ModelSetup(state, actions, modifier)
        }
    }
}

@Composable
private fun ModelSetup(
    state: SettingsState,
    actions: SettingsActions,
    modifier: Modifier,
) {
    Column(
        modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Spacing.L.dp),
        verticalArrangement = Arrangement.spacedBy(Spacing.L.dp),
    ) {
        Text(
            "Model",
            color = MaterialTheme.colorScheme.onBackground,
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
        )
        FormField(
            label = "Address",
            value = state.address,
            onValueChange = { actions.onChange(SettingsField.Address, it) },
            tag = "settings-address",
            error = REQUIRED.takeIf { state.addressMissing },
            keyboard = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
        )
        FormField(
            label = "Model name",
            value = state.model,
            onValueChange = { actions.onChange(SettingsField.ModelName, it) },
            tag = "settings-model",
            error = REQUIRED.takeIf { state.modelMissing },
            keyboard = KeyboardOptions(autoCorrectEnabled = false, capitalization = KeyboardCapitalization.None),
        )
        FormField(
            label = "API key",
            value = state.keyEntry,
            onValueChange = { actions.onChange(SettingsField.Key, it) },
            tag = "settings-key",
            hint = KEY_SAVED_HINT.takeIf { state.keyIsSet && state.keyEntry.isEmpty() },
            placeholder = KEY_DOTS.takeIf { state.keyIsSet },
            keyboard = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
            visualTransformation = PasswordVisualTransformation(),
        )
        ActionRow(state, actions)
        Notices(state)
    }
}

@Composable
private fun ActionRow(
    state: SettingsState,
    actions: SettingsActions,
) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp)) {
        Button(
            onClick = actions.onSave,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.testTag("settings-save"),
        ) {
            Text("Save")
        }
        FilledTonalButton(
            onClick = actions.onTest,
            enabled = state.canTest,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.testTag("settings-test"),
        ) {
            if (state.testing) CircularProgressIndicator(Modifier.padding(end = Spacing.S.dp))
            Text("Test")
        }
        if (state.keyIsSet) {
            FilledTonalButton(
                onClick = actions.onRemoveKey,
                shape = MaterialTheme.shapes.medium,
                colors =
                    ButtonDefaults.filledTonalButtonColors(
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                modifier = Modifier.testTag("settings-remove-key"),
            ) {
                Text("Remove key")
            }
        }
    }
}

@Composable
private fun Notices(state: SettingsState) {
    Column(verticalArrangement = Arrangement.spacedBy(Spacing.S.dp)) {
        if (state.saved) MessageLine(MessageKind.Done, "Saved.", Modifier.testTag("settings-saved"))
        state.failure?.let { MessageLine(MessageKind.Error, it, Modifier.testTag("settings-failure")) }
        val check = state.check
        if (check is SettingsCheck.Shown) {
            Text(
                "Checks of the model",
                color = MaterialTheme.colorScheme.onBackground,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium,
            )
            CheckRow("Answers", check.answers, "settings-check-answers")
            CheckRow("Uses tools", check.tools, "settings-check-tools")
        }
        if (state.keyIsSet) Box(Modifier.testTag("settings-key-is-set"))
    }
}

@Composable
private fun CheckRow(
    name: String,
    line: CheckLine,
    tag: String,
) {
    val kind =
        when (line.status) {
            CheckStatus.Checking -> MessageKind.Checking
            CheckStatus.Works -> MessageKind.Done
            CheckStatus.Failed -> MessageKind.Error
            CheckStatus.Skipped -> MessageKind.Skipped
        }
    val words =
        when (line.status) {
            CheckStatus.Checking -> "checking"
            CheckStatus.Works -> "works"
            CheckStatus.Failed -> line.reason.orEmpty()
            CheckStatus.Skipped -> "skipped"
        }
    MessageLine(kind, "$name: $words", Modifier.testTag(tag))
}
