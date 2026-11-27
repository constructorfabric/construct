package cf.construct.mobile.feature.dashboard

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Colors

/** The small sizes of the Dashboard page that the shared spacing steps do not have. */
internal object DashboardSizes {
    /** The gap between a title and the line under it. */
    val rowGap = 2.dp

    /** The line around a card. */
    val stroke = 1.dp
}

/** A colour role of the shared tokens, in the theme the phone is in now. */
@Composable
internal fun chartColor(role: ColorRole): Color =
    Color((if (isSystemInDarkTheme()) Colors.dark else Colors.light).getValue(role))
