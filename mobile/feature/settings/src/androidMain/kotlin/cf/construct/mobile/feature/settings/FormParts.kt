package cf.construct.mobile.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/*
 * The two parts the model setup makes from the agreed look: a form row (label, field, hint or error) and a
 * message line (an icon and plain text, no box). The sizes of the look are named here once.
 */

private val fieldHeight = 44.dp
private val iconSize = 16.dp
private val ringWidth = 2.dp
private val borderWidth = 1.dp

/** What a message line says about, which gives it its icon and colour. */
internal enum class MessageKind { Error, Notice, Done, Checking, Skipped }

/** One line of a message: an icon and text, as in the Chat page, with no box around it. */
@Composable
internal fun MessageLine(
    kind: MessageKind,
    text: String,
    modifier: Modifier = Modifier,
) {
    val color =
        if (kind == MessageKind.Error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when (kind) {
            MessageKind.Error -> {
                LineIcon(Icons.Filled.Warning, color)
            }

            MessageKind.Notice, MessageKind.Done -> {
                LineIcon(Icons.Filled.Check, color)
            }

            MessageKind.Checking -> {
                CircularProgressIndicator(Modifier.size(iconSize), color = color, strokeWidth = ringWidth)
            }

            MessageKind.Skipped -> {
                Box(Modifier.size(iconSize).border(borderWidth, color, CircleShape))
            }
        }
        Text(text, color = color, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun LineIcon(
    icon: ImageVector,
    color: androidx.compose.ui.graphics.Color,
) {
    Icon(icon, contentDescription = null, Modifier.size(iconSize), tint = color)
}

/**
 * A form row: the label above, a text field 44 high with a 12 radius, and under it the hint, or the
 * error in its place.
 */
@Composable
internal fun FormField(
    label: String,
    value: String,
    onValueChange: (String) -> Unit,
    tag: String,
    modifier: Modifier = Modifier,
    hint: String? = null,
    error: String? = null,
    placeholder: String? = null,
    keyboard: KeyboardOptions = KeyboardOptions.Default,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    var focused by remember { mutableStateOf(false) }
    val scheme = MaterialTheme.colorScheme
    val border =
        when {
            error != null -> BorderStroke(ringWidth, scheme.error)
            focused -> BorderStroke(ringWidth, scheme.primary)
            else -> BorderStroke(borderWidth, scheme.outlineVariant)
        }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Spacing.XS.dp)) {
        Text(
            label,
            color = if (error != null) scheme.error else scheme.onBackground,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            modifier =
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = fieldHeight)
                    .onFocusChanged { focused = it.isFocused }
                    .testTag(tag),
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onBackground),
            cursorBrush = SolidColor(scheme.primary),
            keyboardOptions = keyboard,
            visualTransformation = visualTransformation,
            decorationBox = { inner ->
                Box(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = fieldHeight)
                        .border(border, MaterialTheme.shapes.medium)
                        .padding(horizontal = Spacing.M.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (value.isEmpty() && placeholder != null) {
                        Text(placeholder, color = scheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                    }
                    inner()
                }
            },
        )
        when {
            error != null -> Text(error, color = scheme.error, style = MaterialTheme.typography.labelMedium)
            hint != null -> Text(hint, color = scheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
        }
    }
}
