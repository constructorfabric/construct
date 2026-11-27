package cf.construct.mobile.feature.dashboard

/** What the person can do on the page, so that the screen names no state holder. */
data class DashboardActions(
    val onSpan: (DashboardSpan) -> Unit,
    val onPrevious: () -> Unit,
    val onNext: () -> Unit,
    val onToday: () -> Unit,
    val onPick: (CalendarDay) -> Unit,
)
