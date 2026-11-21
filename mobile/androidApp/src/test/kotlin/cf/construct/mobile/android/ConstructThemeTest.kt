package cf.construct.mobile.android

import androidx.compose.ui.graphics.Color
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Colors
import kotlin.test.Test
import kotlin.test.assertEquals

class ConstructThemeTest {
    @Test
    fun theLightSchemeTakesEverySlotFromTheLightTokens() {
        assertEquals(expected(Colors.light), slots(colorSchemeOf(dark = false)))
    }

    @Test
    fun theDarkSchemeTakesEverySlotFromTheDarkTokens() {
        assertEquals(expected(Colors.dark), slots(colorSchemeOf(dark = true)))
    }

    private fun slots(scheme: androidx.compose.material3.ColorScheme) =
        mapOf(
            "primary" to scheme.primary,
            "onPrimary" to scheme.onPrimary,
            "secondary" to scheme.secondary,
            "onSecondary" to scheme.onSecondary,
            "background" to scheme.background,
            "onBackground" to scheme.onBackground,
            "surface" to scheme.surface,
            "onSurface" to scheme.onSurface,
            "surfaceVariant" to scheme.surfaceVariant,
            "onSurfaceVariant" to scheme.onSurfaceVariant,
            "error" to scheme.error,
            "onError" to scheme.onError,
            "outline" to scheme.outline,
            "outlineVariant" to scheme.outlineVariant,
        )

    private fun expected(roles: Map<ColorRole, Long>): Map<String, Color> {
        fun role(role: ColorRole) = Color(roles.getValue(role))
        return mapOf(
            "primary" to role(ColorRole.PRIMARY),
            "onPrimary" to role(ColorRole.PRIMARY_FOREGROUND),
            "secondary" to role(ColorRole.SECONDARY),
            "onSecondary" to role(ColorRole.SECONDARY_FOREGROUND),
            "background" to role(ColorRole.BACKGROUND),
            "onBackground" to role(ColorRole.FOREGROUND),
            "surface" to role(ColorRole.SURFACE),
            "onSurface" to role(ColorRole.CARD_FOREGROUND),
            "surfaceVariant" to role(ColorRole.MUTED),
            "onSurfaceVariant" to role(ColorRole.MUTED_FOREGROUND),
            "error" to role(ColorRole.DESTRUCTIVE),
            "onError" to role(ColorRole.DESTRUCTIVE_FOREGROUND),
            "outline" to role(ColorRole.BORDER_STRONG),
            "outlineVariant" to role(ColorRole.BORDER),
        )
    }
}
