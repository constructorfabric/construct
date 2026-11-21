package cf.construct.mobile.core.designsystem

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ColorsTest {
    @Test
    fun everyRoleHasAValueInBothThemes() {
        assertEquals(ColorRole.entries.toSet(), Colors.light.keys)
        assertEquals(ColorRole.entries.toSet(), Colors.dark.keys)
    }

    @Test
    fun everyValueIsOpaque() {
        val all = Colors.light.values + Colors.dark.values
        assertTrue(all.all { it in OPAQUE_MIN..OPAQUE_MAX })
    }

    @Test
    fun theLightThemeHasALightPageAndDarkText() {
        assertTrue(brightness(Colors.light, ColorRole.BACKGROUND) > brightness(Colors.dark, ColorRole.BACKGROUND))
        assertTrue(brightness(Colors.light, ColorRole.FOREGROUND) < brightness(Colors.dark, ColorRole.FOREGROUND))
    }

    @Test
    fun textStandsOutFromThePageInBothThemes() {
        for (theme in listOf(Colors.light, Colors.dark)) {
            assertTrue(brightness(theme, ColorRole.FOREGROUND) != brightness(theme, ColorRole.BACKGROUND))
        }
    }

    /** The sum of the three colour channels; only the order of two values matters. */
    private fun brightness(
        theme: Map<ColorRole, Long>,
        role: ColorRole,
    ): Long {
        val value = theme.getValue(role)
        return (value shr RED_SHIFT and CHANNEL) + (value shr GREEN_SHIFT and CHANNEL) + (value and CHANNEL)
    }

    private companion object {
        const val OPAQUE_MIN = 0xFF000000
        const val OPAQUE_MAX = 0xFFFFFFFF
        const val RED_SHIFT = 16
        const val GREEN_SHIFT = 8
        const val CHANNEL = 0xFFL
    }
}
