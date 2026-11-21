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
    fun theThemesDifferWhereTheyShould() {
        assertEquals(0xFFF1F5F9, Colors.light.getValue(ColorRole.BACKGROUND))
        assertEquals(0xFF0F172A, Colors.dark.getValue(ColorRole.BACKGROUND))
    }

    @Test
    fun spacingStepsGrow() {
        val steps = listOf(Spacing.XS, Spacing.S, Spacing.M, Spacing.L, Spacing.XL, Spacing.XXL)
        assertEquals(steps.sorted(), steps)
        assertEquals(steps.size, steps.toSet().size)
    }

    private companion object {
        const val OPAQUE_MIN = 0xFF000000
        const val OPAQUE_MAX = 0xFFFFFFFF
    }
}
