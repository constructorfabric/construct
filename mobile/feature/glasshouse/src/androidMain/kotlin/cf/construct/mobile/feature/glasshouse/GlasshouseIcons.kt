package cf.construct.mobile.feature.glasshouse

import cf.construct.mobile.core.designsystem.LUCIDE_CIRCLE
import cf.construct.mobile.core.designsystem.lucide

internal object GlasshouseIcons {
    val lock =
        lucide(
            "lock",
            "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z",
            "M7 11V7a5 5 0 0 1 10 0v4",
        )
    val reload = lucide("rotate-cw", "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8", "M21 3v5h-5")
    val clear = lucide("x", "M18 6 6 18", "m6 6 12 12")
    val info = lucide("info", LUCIDE_CIRCLE, "M12 11v5", "M12 8h.01")
    val alert = lucide("circle-alert", LUCIDE_CIRCLE, "M12 8v4", "M12 16h.01")
}
