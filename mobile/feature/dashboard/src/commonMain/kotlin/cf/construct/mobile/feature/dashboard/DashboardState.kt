package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.Widget

/**
 * What the page counts: one day, or the week of that day (Monday to Sunday). These are the two periods the
 * computer sends numbers for.
 */
enum class DashboardSpan { DAY, WEEK }

/** A date as the computer's calendar names it: no time zone, so that Swift and Kotlin read it the same. */
data class CalendarDay(
    val year: Int,
    val month: Int,
    val day: Int,
)

/** One widget of the document with what the copy holds for it. */
data class WidgetState(
    val widget: Widget,
    /** What the widget counts here: the page's span, or a single day for the summary of a day. */
    val span: DashboardSpan,
    /** The numbers of the copy for the widget over [span], or null when the computer has not sent them. */
    val numbers: DashboardNumbers?,
    /** What the widget counts the time of: its own source, the document's, or the AI sessions. */
    val source: Source,
)

/** Everything the Dashboard page shows, in one immutable value. */
data class DashboardState(
    val loading: Boolean = true,
    /** Why the last read failed, or null. */
    val failure: String? = null,
    /** Whether the copy holds the computer's dashboard. Before the first copy, and after an erase, it does not. */
    val hasDashboard: Boolean = false,
    val title: String = "",
    val span: DashboardSpan = DashboardSpan.DAY,
    /** The day the page stands on, in the computer's time zone: today until the person moves. */
    val day: CalendarDay? = null,
    /** The first and the last day of what the page counts: the same day, or Monday and Sunday. */
    val first: CalendarDay? = null,
    val last: CalendarDay? = null,
    /** Today in the computer's time zone. */
    val today: CalendarDay? = null,
    /** The computer's time zone, such as `Europe/Berlin`. Not `zone`: Swift reads that as `NSObject.zone()`. */
    val zoneName: String? = null,
    /** Whether the page stands on today, or on the week of today when it counts weeks. */
    val onToday: Boolean = true,
    /** Whether a day or a week after the one shown can hold numbers: nothing is counted for the future. */
    val canGoNext: Boolean = false,
    /** When the copy was last brought up to date (RFC 3339), or null if it never was. */
    val updatedAt: String? = null,
    /** The widgets of the document in the order of its grid, left to right and top to bottom. */
    val widgets: List<WidgetState> = emptyList(),
) {
    /** Nothing in the copy: the page tells the person to connect a computer. */
    val isEmpty: Boolean get() = !loading && failure == null && !hasDashboard

    /** The copy holds numbers for none of the widgets on this day or week. */
    val nothingSent: Boolean get() = widgets.isNotEmpty() && widgets.all { it.numbers == null }
}
