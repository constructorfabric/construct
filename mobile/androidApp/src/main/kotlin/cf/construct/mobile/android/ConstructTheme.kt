package cf.construct.mobile.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Colors

/*
 * The Android theme. The colours come from the tokens both apps share. The text sizes and the radii
 * below, and the control sizes in ControlSizes.kt, are placeholders until the design of the mobile apps
 * sets them.
 */

private val constructTypography =
    Typography(
        titleLarge = TextStyle(fontSize = 22.sp, lineHeight = 28.sp),
        bodyLarge = TextStyle(fontSize = 16.sp, lineHeight = 24.sp),
        bodyMedium = TextStyle(fontSize = 14.sp, lineHeight = 20.sp),
        labelMedium = TextStyle(fontSize = 12.sp, lineHeight = 16.sp),
    )

private val constructShapes =
    Shapes(
        small = RoundedCornerShape(4.dp),
        medium = RoundedCornerShape(8.dp),
        large = RoundedCornerShape(12.dp),
    )

/** Maps the shared colour roles to the Material colour scheme. */
internal fun colorSchemeOf(dark: Boolean): ColorScheme {
    val roles = if (dark) Colors.dark else Colors.light

    fun role(role: ColorRole) = Color(roles.getValue(role))
    val scheme =
        if (dark) {
            androidx.compose.material3.darkColorScheme()
        } else {
            androidx.compose.material3
                .lightColorScheme()
        }
    return scheme.copy(
        primary = role(ColorRole.PRIMARY),
        onPrimary = role(ColorRole.PRIMARY_FOREGROUND),
        secondary = role(ColorRole.SECONDARY),
        onSecondary = role(ColorRole.SECONDARY_FOREGROUND),
        background = role(ColorRole.BACKGROUND),
        onBackground = role(ColorRole.FOREGROUND),
        surface = role(ColorRole.SURFACE),
        onSurface = role(ColorRole.CARD_FOREGROUND),
        surfaceVariant = role(ColorRole.MUTED),
        onSurfaceVariant = role(ColorRole.MUTED_FOREGROUND),
        error = role(ColorRole.DESTRUCTIVE),
        onError = role(ColorRole.DESTRUCTIVE_FOREGROUND),
        outline = role(ColorRole.BORDER_STRONG),
        outlineVariant = role(ColorRole.BORDER),
    )
}

/** Follows the system light or dark theme. */
@Composable
fun ConstructTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = colorSchemeOf(dark = isSystemInDarkTheme()),
        typography = constructTypography,
        shapes = constructShapes,
        content = content,
    )
}
