package cf.construct.mobile.android

import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Sizes of controls, in dp. A placeholder scale. */
data class ControlSizes(
    val small: Dp = 32.dp,
    val medium: Dp = 40.dp,
    val large: Dp = 48.dp,
)

val LocalControlSizes = staticCompositionLocalOf { ControlSizes() }
