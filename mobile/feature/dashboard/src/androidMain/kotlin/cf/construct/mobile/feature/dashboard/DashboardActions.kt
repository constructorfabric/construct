package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.model.GroupMinutes

/** What the person can do on the page, so that the screen names no state holder. */
data class DashboardActions(
    val onSpan: (DashboardSpan) -> Unit,
    val onPrevious: () -> Unit,
    val onNext: () -> Unit,
    val onToday: () -> Unit,
    val onPick: (CalendarDay) -> Unit,
    /** The sessions the copy holds behind a row of a table. */
    val sessionsOf: (WidgetState, GroupMinutes) -> RowSessions,
)
