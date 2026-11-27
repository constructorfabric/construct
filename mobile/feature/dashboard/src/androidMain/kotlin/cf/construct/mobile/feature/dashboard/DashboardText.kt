package cf.construct.mobile.feature.dashboard

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.WidgetKind
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import kotlin.math.roundToInt

/** The words of the Dashboard page, as the iPhone page says them. The phone counts nothing: it words the copy. */
internal object DashboardText {
    const val NOT_SENT_DAY = "Your computer has not sent the numbers for this day."
    const val NOT_SENT_WEEK = "Your computer has not sent the numbers for this week."
    const val EMPTY_TITLE = "Nothing from your computer yet"
    const val EMPTY_TEXT = "Pair this phone with your computer on the Settings page, and its dashboard shows here."
    const val NO_TABLE = "No time in this period."
    const val NO_SUMMARY = "No time on this day."
    const val NOT_UPDATED = "Not updated from your computer yet."

    /** Whole minutes the way the computer's dashboard shows time: `45 min`, `1 h 5 min`. */
    fun minutes(value: Int) =
        if (value <
            MINUTES_PER_HOUR
        ) {
            "$value min"
        } else {
            "${value / MINUTES_PER_HOUR} h ${value % MINUTES_PER_HOUR} min"
        }

    fun sessions(count: Int) = if (count == 1) "1 session" else "$count sessions"

    fun hiddenProjects(count: Int) =
        if (count == 1) "1 project is hidden on your computer." else "$count projects are hidden on your computer."

    fun notSent(span: DashboardSpan) = if (span == DashboardSpan.WEEK) NOT_SENT_WEEK else NOT_SENT_DAY

    fun title(
        kind: WidgetKind,
        groupBy: cf.construct.mobile.core.model.GroupBy?,
    ) = when (kind) {
        WidgetKind.TOTAL -> "Time"
        WidgetKind.BARS_PER_DAY -> "Time per day"
        WidgetKind.DAY_SUMMARY -> "End of day"
        WidgetKind.TABLE -> "Time by ${groupName(groupBy)}"
    }

    private fun groupName(groupBy: cf.construct.mobile.core.model.GroupBy?) =
        when (groupBy ?: cf.construct.mobile.core.model.GroupBy.PROJECT) {
            cf.construct.mobile.core.model.GroupBy.PROJECT -> "project"
            cf.construct.mobile.core.model.GroupBy.TICKET -> "ticket"
            cf.construct.mobile.core.model.GroupBy.TOOL -> "tool"
            cf.construct.mobile.core.model.GroupBy.COLLECTION -> "group"
            cf.construct.mobile.core.model.GroupBy.DAY -> "day"
            cf.construct.mobile.core.model.GroupBy.APP -> "app"
            cf.construct.mobile.core.model.GroupBy.SITE -> "website"
        }

    fun sourceName(source: cf.construct.mobile.core.model.Source) =
        when (source) {
            cf.construct.mobile.core.model.Source.AI_SESSIONS -> "AI sessions"
            cf.construct.mobile.core.model.Source.ON_SCREEN -> "On screen"
        }

    /** "Updated Nov 26, 2026, 10:30 AM", or the words for a copy that has no time. */
    fun updated(saved: String?): String {
        val moment = saved?.let { runCatching { java.time.Instant.parse(it) }.getOrNull() } ?: return NOT_UPDATED
        val format = DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
        return "Updated ${format.withLocale(Locale.getDefault()).format(moment.atZone(ZoneId.systemDefault()))}"
    }

    private const val MINUTES_PER_HOUR = 60
}

/** The days of the computer's calendar as the phone words them: its own language and order, the computer's days. */
internal class DashboardDates(
    private val locale: Locale = Locale.getDefault(),
) {
    private fun date(day: CalendarDay) = LocalDate.of(day.year, day.month, day.day)

    private fun label(label: String): LocalDate? = runCatching { LocalDate.parse(label) }.getOrNull()

    private fun skeleton(
        skeleton: String,
        date: LocalDate,
    ): String {
        val format =
            android.icu.text.DateFormat
                .getInstanceForSkeleton(skeleton, locale)
        // The date is read in UTC, at noon, so that no zone of the phone moves it to another day.
        format.timeZone = android.icu.util.TimeZone.GMT_ZONE
        return format.format(java.util.Date.from(date.atStartOfDay(ZoneOffset.UTC).toInstant().plusSeconds(NOON)))
    }

    /** `Wed, Nov 18`. */
    fun text(day: CalendarDay) = skeleton("EEEMMMd", date(day))

    /** `Nov 16 – Nov 22`, or the one day when the first is the last. */
    fun range(
        first: CalendarDay,
        last: CalendarDay,
    ) = if (first == last) text(first) else "${skeleton("MMMd", date(first))} – ${skeleton("MMMd", date(last))}"

    /** `Mon`, for the foot of a bar. */
    fun weekday(label: String) = label(label)?.let { skeleton("EEE", it) } ?: label

    /** `Monday, Nov 16`, for a bar read out. */
    fun spoken(label: String) = label(label)?.let { skeleton("EEEEMMMd", it) } ?: label

    private companion object {
        const val NOON = 12 * 3600L
    }
}
