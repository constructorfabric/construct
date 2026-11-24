package cf.construct.mobile.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Info
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.connection.PAIRING_CODE_LENGTH
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.feature.status.ComputerOperation
import cf.construct.mobile.feature.status.ConnectionStatus
import cf.construct.mobile.feature.status.ConnectionStatusKind
import cf.construct.mobile.feature.status.dotColor
import cf.construct.mobile.feature.status.lastContactText

@Composable
internal fun ComputerSettingsRow(
    connection: ConnectionStatus,
    open: () -> Unit,
) {
    ListItem(
        headlineContent = { Text("Computer") },
        supportingContent = connection.endpoint?.let { address -> { Text(address) } },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(connection.label, style = MaterialTheme.typography.bodyMedium)
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null)
            }
        },
        modifier = Modifier.clickable(onClick = open).testTag("open-computer"),
    )
}

@Composable
internal fun ComputerScreen(
    connection: ConnectionStatus,
    operation: ComputerOperation,
    open: (ComputerPage) -> Unit,
    retry: () -> Unit,
    unpair: () -> Unit,
) {
    var confirming by rememberSaveable { mutableStateOf(false) }
    var replacing by rememberSaveable { mutableStateOf(false) }
    val unpaired = connection.kind == ConnectionStatusKind.NotPaired
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Spacing.L.dp),
        verticalArrangement =
            Arrangement.spacedBy(Spacing.L.dp, if (unpaired) Alignment.CenterVertically else Alignment.Top),
        horizontalAlignment = if (unpaired) Alignment.CenterHorizontally else Alignment.Start,
    ) {
        when (connection.kind) {
            ConnectionStatusKind.Checking -> {
                CircularProgressIndicator()
                Text(connection.kind.title)
            }

            ConnectionStatusKind.NotPaired -> {
                Icon(
                    PageIcons.unlink,
                    contentDescription = null,
                    modifier = Modifier.size(LocalControlSizes.current.iconLarge),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    connection.kind.title,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                PairingChoices(open, enabled = !operation.busy, textAlign = TextAlign.Center)
            }

            else -> {
                ComputerDetails(connection, retry, operation.busy)
                if (connection.kind != ConnectionStatusKind.Connected) {
                    TextButton(onClick = { replacing = !replacing }, enabled = !operation.busy) { Text("Pair again") }
                    if (replacing) PairingChoices(open, enabled = !operation.busy)
                }
                TextButton(onClick = { confirming = true }, enabled = !operation.busy) {
                    Text("Unpair", color = MaterialTheme.colorScheme.error)
                }
            }
        }
        OperationFeedback(operation)
    }
    if (confirming) {
        ConfirmUnpair(onDismiss = { confirming = false }, unpair)
    }
}

@Composable
private fun ConfirmUnpair(
    onDismiss: () -> Unit,
    unpair: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Unpair this computer?") },
        text = { Text("This removes the computer’s copied data. Chats started on this phone stay.") },
        confirmButton = {
            TextButton(
                onClick = {
                    onDismiss()
                    unpair()
                },
                modifier = Modifier.testTag("confirm-unpair"),
            ) { Text("Unpair") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun PairingChoices(
    open: (ComputerPage) -> Unit,
    enabled: Boolean,
    textAlign: TextAlign = TextAlign.Start,
) {
    Text(
        "Open Settings on your computer and choose Pair a phone.",
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = textAlign,
    )
    Button(onClick = { open(ComputerPage.Scan) }, enabled = enabled) { Text("Scan QR code") }
    OutlinedButton(onClick = { open(ComputerPage.Manual) }, enabled = enabled) { Text("Enter address and code") }
}

@Composable
private fun ComputerDetails(
    connection: ConnectionStatus,
    retry: () -> Unit,
    busy: Boolean,
) {
    val connected = connection.kind == ConnectionStatusKind.Connected
    val dot = connection.kind.dotColor()
    if (!connected) {
        ComputerNotice(
            connection.kind.title + "." +
                connection.kind.detail
                    ?.let { " $it" }
                    .orEmpty(),
        )
    }
    Column {
        connection.endpoint?.let { ComputerDetail("Paired computer", it) }
        ComputerDetail("Connection", connection.label, lastContactText(connection.lastContactMillis), dot)
        ComputerDetail(
            "Agent",
            if (connected) "Running" else "Unknown",
            "Known again when the computer answers.".takeUnless { connected },
            if (connected) dot else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    if (connection.kind.canRetry) {
        OutlinedButton(
            onClick = retry,
            enabled = !busy,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.fillMaxWidth().heightIn(min = LocalControlSizes.current.outlinedButton),
        ) { Text("Try again") }
    }
}

@Composable
private fun ComputerDetail(
    title: String,
    value: String,
    detail: String? = null,
    dot: Color? = null,
) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = Spacing.M.dp),
        horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            if (detail != null) {
                Text(
                    detail,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        Row(
            modifier = Modifier.weight(1f),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp, Alignment.End),
        ) {
            if (dot != null) Box(Modifier.size(LocalControlSizes.current.dot).background(dot, CircleShape))
            Text(value, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.End)
        }
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
}

private val ConnectionStatus.label: String
    get() =
        when (kind) {
            ConnectionStatusKind.Unreachable, ConnectionStatusKind.NoNetwork -> "Not reachable"
            else -> kind.title
        }

@Composable
internal fun ManualPairingScreen(
    operation: ComputerOperation,
    pair: (String, String) -> Unit,
    allowNetwork: () -> Unit,
) {
    var address by rememberSaveable { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    val focus = LocalFocusManager.current
    LaunchedEffect(operation.needsFreshCode) {
        if (operation.needsFreshCode) code = ""
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Spacing.L.dp),
        verticalArrangement = Arrangement.spacedBy(Spacing.L.dp),
    ) {
        Text("Enter the address and eight-digit code shown on your computer.")
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            label = { Text("Computer address") },
            enabled = !operation.busy,
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
            modifier = Modifier.fillMaxWidth().testTag("computer-address"),
        )
        OutlinedTextField(
            value = code,
            onValueChange = { code = it.filter { digit -> digit in '0'..'9' }.take(PAIRING_CODE_LENGTH) },
            label = { Text("Pairing code") },
            enabled = !operation.busy,
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
            visualTransformation = PasswordVisualTransformation(),
            modifier = Modifier.fillMaxWidth().testTag("computer-code"),
        )
        Button(
            onClick = {
                focus.clearFocus()
                pair(address, code)
            },
            enabled = address.isNotBlank() && code.length == PAIRING_CODE_LENGTH && !operation.busy,
        ) { Text("Pair") }
        OperationFeedback(operation, allowNetwork)
    }
}

@Composable
internal fun OperationFeedback(
    operation: ComputerOperation,
    allowNetwork: () -> Unit = {},
) {
    if (operation.busy) CircularProgressIndicator()
    operation.message?.let { ComputerNotice(it, isError = true) }
    if (operation.needsNetworkPermission) {
        TextButton(onClick = allowNetwork) { Text("Allow local network access") }
    }
}

@Composable
private fun ComputerNotice(
    message: String,
    isError: Boolean = false,
) {
    val color = if (isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant
    Row(horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp)) {
        Icon(
            Icons.Filled.Info,
            contentDescription = null,
            modifier = Modifier.size(LocalControlSizes.current.noticeIcon),
            tint = color,
        )
        Text(message, style = MaterialTheme.typography.bodyMedium, color = color)
    }
}
