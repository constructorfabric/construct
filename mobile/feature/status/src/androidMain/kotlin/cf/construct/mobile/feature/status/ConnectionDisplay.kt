package cf.construct.mobile.feature.status

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Colors

/** The dot of a state: green when connected, red when the computer cannot be reached, grey while unknown. */
@Composable
fun ConnectionStatusKind.dotColor(): Color =
    when (this) {
        ConnectionStatusKind.Connected -> {
            successColor()
        }

        ConnectionStatusKind.Checking, ConnectionStatusKind.NotPaired -> {
            MaterialTheme.colorScheme.outline
        }

        else -> {
            MaterialTheme.colorScheme.error
        }
    }

/** The green of a thing that went well: the dot of a connection that works, the check of a step that is done. */
@Composable
internal fun successColor(): Color {
    val colors = if (isSystemInDarkTheme()) Colors.dark else Colors.light
    return Color(colors.getValue(ColorRole.SUCCESS))
}
