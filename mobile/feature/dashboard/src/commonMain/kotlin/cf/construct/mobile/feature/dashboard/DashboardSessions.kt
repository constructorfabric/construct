package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.GroupSession
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime
import kotlin.time.Instant

/** One session behind a row, with its times in the computer's time zone: the date, and the clock times `09:30`. */
data class SessionLine(
    val date: String,
    val start: String,
    val end: String,
    /** Its share of the row in whole minutes; a minute that sessions share is split between them. */
    val minutes: Int,
    val tool: String,
    val project: String,
    val ticket: String,
    /** The first characters of the id of the session. */
    val session: String,
)

/**
 * The sessions behind one row of a table.
 *
 * @property sent false when the computer has not sent the sessions for this day or week (an older computer, or a
 *   day older than it sends them for): the page says so, and [lines] is empty.
 * @property sharedMinutes whether some sessions of the row overlap in time, so that the minutes shown are shares.
 */
data class RowSessions(
    val sent: Boolean,
    val lines: List<SessionLine>,
    val rowMinutes: Int,
    val sharedMinutes: Boolean,
) {
    /** The minutes of the sessions added up; it is [rowMinutes] when the computer's numbers are whole. */
    val sessionMinutes: Int get() = lines.sumOf { it.minutes }
}

internal fun rowSessions(
    state: WidgetState,
    row: GroupMinutes,
    zone: TimeZone,
): RowSessions {
    val sent = state.numbers?.sessionsByGroup?.get(row.key)
    if (sent == null) {
        return RowSessions(sent = false, lines = emptyList(), rowMinutes = row.minutes, sharedMinutes = false)
    }
    val moments = sent.map { Instant.parse(it.start) to Instant.parse(it.end) }
    return RowSessions(
        sent = true,
        lines = sent.map { it.line(zone) },
        rowMinutes = row.minutes,
        sharedMinutes = moments.overlap(),
    )
}

private fun GroupSession.line(zone: TimeZone): SessionLine {
    val from = Instant.parse(start).toLocalDateTime(zone)
    val to = Instant.parse(end).toLocalDateTime(zone)
    return SessionLine(
        date = from.date.toString(),
        start = clock(from.hour, from.minute),
        end = clock(to.hour, to.minute),
        minutes = minutes,
        tool = tool,
        project = project,
        ticket = ticket,
        session = session,
    )
}

private fun clock(
    hour: Int,
    minute: Int,
) = "${hour.toString().padStart(2, '0')}:${minute.toString().padStart(2, '0')}"

/** Whether any two spans share a moment; the spans come the earliest first. */
private fun List<Pair<Instant, Instant>>.overlap(): Boolean {
    var latest: Instant? = null
    for ((from, to) in sortedBy { it.first }) {
        if (latest != null && from < latest) return true
        latest = if (latest == null || to > latest) to else latest
    }
    return false
}
