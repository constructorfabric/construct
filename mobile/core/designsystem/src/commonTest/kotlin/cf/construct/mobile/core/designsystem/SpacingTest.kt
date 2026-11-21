package cf.construct.mobile.core.designsystem

import kotlin.test.Test
import kotlin.test.assertEquals

class SpacingTest {
    @Test
    fun theStepsGrowAndNoTwoAreEqual() {
        val steps = listOf(Spacing.XS, Spacing.S, Spacing.M, Spacing.L, Spacing.XL, Spacing.XXL)
        assertEquals(steps.sorted().distinct(), steps)
    }
}
