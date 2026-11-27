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

/**
 * Shows [state] and loads nothing: the computer's dashboard as the copy on the phone holds it, in one column. The
 * phone counts nothing, so a day or a week the computer sent no numbers for says so and shows no zeros.
 */
@Composable
fun DashboardScreen(
    state: DashboardState,
    actions: DashboardActions,
    modifier: Modifier = Modifier,
) {
    when {
        state.loading && !state.hasDashboard && state.failure == null -> {
            Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        }

        state.isEmpty -> {
            DashboardEmpty(modifier)
        }

        else -> {
            Column(
                modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Spacing.L.dp),
                verticalArrangement = Arrangement.spacedBy(Spacing.L.dp),
            ) {
                state.failure?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (state.hasDashboard) {
                    DashboardHeader(state, actions)
                    if (state.nothingSent) {
                        Text(
                            DashboardText.notSent(state.span),
                            Modifier.testTag("dashboard-not-sent"),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyLarge,
                        )
                    } else {
                        val dates = DashboardDates()
                        state.widgets.forEach { WidgetCard(it, state, dates) }
                    }
                }
            }
        }
    }
}

/** What the page says when the copy holds no dashboard: before the first copy, and after "Delete everything". */
@Composable
private fun DashboardEmpty(modifier: Modifier) {
    Column(
        modifier.fillMaxSize().padding(horizontal = Spacing.XL.dp).testTag("dashboard-empty"),
        verticalArrangement = Arrangement.spacedBy(Spacing.M.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            DashboardText.EMPTY_TITLE,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
        )
        Text(
            DashboardText.EMPTY_TEXT,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
        )
    }
}

/** The title with the time of the last update, and the controls that move the page. */
@Composable
private fun DashboardHeader(
    state: DashboardState,
    actions: DashboardActions,
) {
    val dates = DashboardDates()
    val weeks = state.span == DashboardSpan.WEEK
    var picking by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Spacing.M.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(DashboardSizes.rowGap)) {
            Text(
                state.title,
                Modifier.semantics { heading() },
                color = MaterialTheme.colorScheme.onBackground,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                DashboardText.updated(state.updatedAt),
                Modifier.testTag("dashboard-updated"),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            val spans = listOf(DashboardSpan.DAY to "Day", DashboardSpan.WEEK to "Week")
            spans.forEachIndexed { index, (span, label) ->
                SegmentedButton(
                    selected = state.span == span,
                    onClick = { actions.onSpan(span) },
                    shape = SegmentedButtonDefaults.itemShape(index, spans.size),
                ) { Text(label) }
            }
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        ) {
            TextButton(
                onClick = actions.onPrevious,
                modifier = Modifier.testTag("dashboard-previous"),
            ) { Text(if (weeks) "Previous week" else "Previous day") }
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                TextButton(onClick = { picking = true }, modifier = Modifier.testTag("dashboard-date")) {
                    Text(rangeText(state, dates), fontWeight = FontWeight.SemiBold)
                }
            }
            TextButton(
                onClick = actions.onNext,
                enabled = state.canGoNext,
                modifier = Modifier.testTag("dashboard-next"),
            ) { Text(if (weeks) "Next week" else "Next day") }
        }
        if (!state.onToday) {
            TextButton(onClick = actions.onToday, modifier = Modifier.testTag("dashboard-today")) { Text("Today") }
        }
    }
    if (picking) {
        DashboardDatePicker(state, actions.onPick) { picking = false }
    }
}

/** The calendar of the computer's days, up to today: nothing is counted for the future. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DashboardDatePicker(
    state: DashboardState,
    onPick: (CalendarDay) -> Unit,
    onClose: () -> Unit,
) {
    val limit = state.today?.toLocalDate()
    val picker =
        rememberDatePickerState(
            initialSelectedDateMillis = (state.day ?: state.today)?.toLocalDate()?.let(::utcMillis),
            selectableDates =
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long) =
                        limit == null || utcTimeMillis <= utcMillis(limit)
                },
        )
    DatePickerDialog(
        onDismissRequest = onClose,
        confirmButton = {
            TextButton(
                onClick = {
                    picker.selectedDateMillis?.let { onPick(calendarDayOf(it)) }
                    onClose()
                },
                modifier = Modifier.testTag("dashboard-calendar-done"),
            ) { Text("Done") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("Cancel") } },
    ) {
        DatePicker(picker, Modifier.testTag("dashboard-calendar"))
    }
}

private fun rangeText(
    state: DashboardState,
    dates: DashboardDates,
): String = state.first?.let { first -> state.last?.let { last -> dates.range(first, last) } }.orEmpty()

private fun utcMillis(date: LocalDate) = date.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()

private fun calendarDayOf(utcMillis: Long): CalendarDay {
    val date =
        java.time.Instant
            .ofEpochMilli(utcMillis)
            .atZone(ZoneOffset.UTC)
            .toLocalDate()
    return CalendarDay(date.year, date.monthValue, date.dayOfMonth)
}

private fun CalendarDay.toLocalDate(): LocalDate? = runCatching { LocalDate.of(year, month, day) }.getOrNull()
