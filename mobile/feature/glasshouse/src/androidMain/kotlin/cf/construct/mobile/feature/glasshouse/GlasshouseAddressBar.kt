package cf.construct.mobile.feature.glasshouse

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing
import kotlinx.coroutines.flow.drop

private val barHeight = 44.dp
private val iconSize = 16.dp
private val progressHeight = 3.dp
private const val ADDRESS_HINT = "Address"
private const val MIN_PROGRESS = 0.05f
private const val PERCENT = 100f

// The tags the tests find the bar's parts by.
const val ADDRESS_TAG = "glasshouse-address"
const val BAR_RELOAD_TAG = "glasshouse-bar-reload"

/**
 * The browser's address bar: one rounded bar with the lock, the host and reload. When the person taps it, it shows
 * the whole address in a text field with a clear button instead of reload; the keyboard's Go key opens the address.
 * A line along the bottom of the bar shows how much of the page has loaded.
 *
 * The text field exists only while the person types. Showing the host is a plain text, so there is no field left
 * to hold the focus (and the keyboard) once the address is opened.
 */
@Composable
internal fun GlasshouseAddressBar(
    state: GlasshouseState,
    progress: Int,
    actions: GlasshouseActions,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    val keyboard = LocalSoftwareKeyboardController.current
    val requester = remember { FocusRequester() }
    val text = rememberTextFieldState()
    var editing by remember { mutableStateOf(false) }
    val secure = state.problem == null && state.address.startsWith("https:", ignoreCase = true)

    val pill = Modifier.fillMaxWidth().height(barHeight).clip(MaterialTheme.shapes.extraLarge)
    Box(modifier.then(pill).background(scheme.secondaryContainer)) {
        Row(Modifier.fillMaxWidth().padding(start = Spacing.M.dp), verticalAlignment = Alignment.CenterVertically) {
            if (secure && !editing) {
                Icon(
                    GlasshouseIcons.lock,
                    contentDescription = "Secure connection",
                    modifier = Modifier.padding(end = Spacing.S.dp).size(iconSize),
                    tint = scheme.onSurfaceVariant,
                )
            }
            Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.CenterStart) {
                if (editing) {
                    AddressField(
                        text,
                        requester,
                        onEdited = actions.onEdited,
                        onGo = {
                            if (actions.onGo(text.text.toString())) {
                                keyboard?.hide()
                                editing = false
                            }
                        },
                        onLeave = {
                            editing = false
                            actions.onEdited()
                        },
                    )
                } else {
                    AddressText(state.address) {
                        text.setTextAndPlaceCursorAtEnd(state.address)
                        editing = true
                    }
                }
            }
            val clear: () -> Unit = {
                text.clearText()
                requester.requestFocus()
            }
            TrailingButton(editing, state.address.isNotEmpty(), clear, actions.onReload)
        }
        if (state.loading) LoadProgress(progress, Modifier.align(Alignment.BottomCenter))
    }
}

/** The host of the page the bar shows, or the hint when there is no address; a tap starts the typing. */
@Composable
private fun AddressText(
    address: String,
    onEdit: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    Box(
        Modifier
            .fillMaxSize()
            .testTag(ADDRESS_TAG)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = "Type an address",
                role = Role.Button,
                onClick = onEdit,
            ),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (address.isEmpty()) {
            Text(ADDRESS_HINT, style = MaterialTheme.typography.bodyLarge, color = scheme.onSurfaceVariant)
        } else {
            Text(
                GlasshouseDisplay.host(address),
                style = MaterialTheme.typography.bodyLarge,
                color = scheme.onBackground,
                maxLines = 1,
            )
        }
    }
}

/**
 * The text field of the address the person types. It takes the focus when it appears, calls [onEdited] when the text
 * changes and [onLeave] when it loses the focus.
 */
@Composable
private fun AddressField(
    text: TextFieldState,
    requester: FocusRequester,
    onEdited: () -> Unit,
    onGo: () -> Unit,
    onLeave: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    // The focus is asked for after the field appears; leaving counts only once it has had the focus.
    var focused by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { requester.requestFocus() }
    LaunchedEffect(text) { snapshotFlow { text.text.toString() }.drop(1).collect { onEdited() } }
    BasicTextField(
        state = text,
        modifier =
            Modifier
                .fillMaxWidth()
                .testTag(ADDRESS_TAG)
                .focusRequester(requester)
                .onFocusChanged {
                    if (it.isFocused) {
                        focused = true
                    } else if (focused) {
                        onLeave()
                    }
                },
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onBackground),
        keyboardOptions =
            KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                autoCorrectEnabled = false,
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Go,
            ),
        onKeyboardAction = { onGo() },
        lineLimits = TextFieldLineLimits.SingleLine,
        cursorBrush = SolidColor(scheme.primary),
    )
}

/** Clear while the person types, Reload when the bar shows a page, and nothing when there is no address. */
@Composable
private fun TrailingButton(
    editing: Boolean,
    hasAddress: Boolean,
    onClear: () -> Unit,
    onReload: () -> Unit,
) {
    val tint = MaterialTheme.colorScheme.onSurfaceVariant
    when {
        editing -> {
            IconButton(onClick = onClear, modifier = Modifier.size(barHeight)) {
                Icon(GlasshouseIcons.clear, "Clear the address", Modifier.size(iconSize), tint = tint)
            }
        }

        hasAddress -> {
            IconButton(onClick = onReload, modifier = Modifier.size(barHeight).testTag(BAR_RELOAD_TAG)) {
                Icon(GlasshouseIcons.reload, "Reload", Modifier.size(iconSize), tint = tint)
            }
        }

        else -> {
            Box(Modifier.size(barHeight))
        }
    }
}

/** The line along the bottom of the bar: how much of the page has loaded. */
@Composable
private fun LoadProgress(
    progress: Int,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    LinearProgressIndicator(
        progress = { (progress / PERCENT).coerceIn(MIN_PROGRESS, 1f) },
        modifier = modifier.fillMaxWidth().height(progressHeight),
        color = scheme.primary,
        trackColor = scheme.outlineVariant,
        gapSize = 0.dp,
        drawStopIndicator = {},
    )
}
