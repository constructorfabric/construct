package cf.construct.mobile.android

import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame

class ThemeSizesTest {
    private val density = Density(1f)
    private val box = Size(400f, 400f)

    private fun radius(shape: CornerBasedShape) = shape.topStart.toPx(box, density)

    @Test
    fun theRadiiAreEightTwelveAndSixteenAndTheLargestIsRound() {
        assertEquals(8f, radius(constructShapes.small))
        assertEquals(12f, radius(constructShapes.medium))
        assertEquals(16f, radius(constructShapes.large))
        assertSame(CircleShape, constructShapes.extraLarge)
    }

    @Test
    fun theTitleBodyAndLabelSizesAreTheAgreedOnes() {
        val type = constructTypography
        assertEquals(22f, type.titleLarge.fontSize.value)
        assertEquals(28f, type.titleLarge.lineHeight.value)
        assertEquals(16f, type.bodyLarge.fontSize.value)
        assertEquals(14f, type.bodyMedium.fontSize.value)
        assertEquals(12f, type.labelMedium.fontSize.value)
    }

    @Test
    fun theControlSizesAreTheAgreedOnes() {
        val sizes = ControlSizes()
        assertEquals(48.dp, sizes.touch)
        assertEquals(56.dp, sizes.button)
        assertEquals(24.dp, sizes.icon)
        assertEquals(40.dp, sizes.iconLarge)
        assertEquals(8.dp, sizes.dot)
    }
}
