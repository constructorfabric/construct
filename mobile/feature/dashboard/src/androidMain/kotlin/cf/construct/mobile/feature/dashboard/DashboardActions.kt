package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.model.Source

/** What the person can do on the page, so that the screen names no state holder. */
data class DashboardActions(
    val onSpan: (DashboardSpan) -> Unit,
    val onSource: (Source) -> Unit,
    val onPrevious: () -> Unit,
    val onNext: () -> Unit,
    val onToday: () -> Unit,
    val onPick: (CalendarDay) -> Unit,
)
