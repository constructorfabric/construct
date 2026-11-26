package cf.construct.mobile.feature.chat

import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.LUCIDE_CIRCLE
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.designsystem.lucide

internal object ChatIcons {
    val back = lucide("arrow-left", "M12 19l-7-7 7-7", "M19 12H5")
    val send = lucide("arrow-up", "M12 19V5", "m5 12 7-7 7 7")
    val stop = lucide("square", "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z")
    val phone =
        lucide("smartphone", "M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z", "M12 18h.01")
    val computer =
        lucide(
            "monitor",
            "M4 3h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
            "M8 23h8",
            "M12 19v4",
        )
    val plus = lucide("plus", "M5 12h14", "M12 5v14")
    val more = lucide("ellipsis", "M5 12h.01", "M12 12h.01", "M19 12h.01")
    val message = lucide("message-square", "M21 16a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z")
    val info = lucide("info", LUCIDE_CIRCLE, "M12 11v5", "M12 8h.01")
    val alert = lucide("circle-alert", LUCIDE_CIRCLE, "M12 8v4", "M12 16h.01")
    val tool = lucide("sparkles", "M9 3l2 6 6 2-6 2-2 6-2-6-6-2 6-2z", "M19 3v6", "M16 6h6", "M18 15v6", "M15 18h6")
}

internal val chatSendIcon = 20.dp
internal val chatEmptyIcon = 40.dp
internal val chatStroke = 1.dp
internal val chatFabClearance = 56.dp + Spacing.XXL.dp
