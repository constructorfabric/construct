package cf.construct.mobile.android

import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls

/** Sizes of controls and icons, in dp. */
data class ControlSizes(
    /** The smallest area a tap may land on. */
    val touch: Dp = 48.dp,
    /** The round action button above the navigation bar. */
    val button: Dp = 56.dp,
    val icon: Dp = 24.dp,
    /** The icon in the middle of an empty page. */
    val iconLarge: Dp = 40.dp,
    /** The dot before the connection state. */
    val dot: Dp = 8.dp,
    val noticeIcon: Dp = SmallControls.icon,
    val outlinedButton: Dp = SmallControls.action,
    /** The line of a small spinner, such as the one in a button. */
    val spinnerStroke: Dp = SmallControls.spinnerStroke,
)

val LocalControlSizes = staticCompositionLocalOf { ControlSizes() }
