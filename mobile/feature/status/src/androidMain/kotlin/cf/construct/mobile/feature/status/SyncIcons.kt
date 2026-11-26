package cf.construct.mobile.feature.status

import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.LUCIDE_CIRCLE
import cf.construct.mobile.core.designsystem.lucide

internal object SyncIcons {
    val circle = lucide("circle", LUCIDE_CIRCLE)
    val check = lucide("circle-check", LUCIDE_CIRCLE, "m9 12 2 2 4-4")
    val monitorOff =
        lucide(
            "monitor-off",
            "M12 17v4",
            "M17 17H4a2 2 0 0 1-2-2V5a2 2 0 0 1 1.184-1.826",
            "m2 2 20 20",
            "M8 21h8",
            "M8.656 3H20a2 2 0 0 1 2 2v10a2 2 0 0 1-.293 1.042",
        )
}

/** The spinner at the top of the loading screen. */
internal val syncLargeSpinner = 40.dp
internal val syncBarHeight = 8.dp
internal val syncBarGap = 0.dp
internal const val SYNC_FADE_MILLIS = 150
