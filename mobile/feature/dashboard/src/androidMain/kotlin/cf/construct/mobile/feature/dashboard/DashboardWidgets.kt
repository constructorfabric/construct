package cf.construct.mobile.feature.dashboard

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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

/** One widget of the document in a card: its title, what it counts and for which days, and its numbers. */
@Composable
internal fun WidgetCard(
    shown: WidgetState,
    state: DashboardState,
    dates: DashboardDates,
    onOpen: (GroupMinutes) -> Unit,
) {
    val id = shown.widget.id
    Card(
        Modifier.fillMaxWidth().testTag("dashboard-widget-$id"),
        shape = MaterialTheme.shapes.medium,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        border = BorderStroke(DashboardSizes.stroke, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(
            Modifier.fillMaxWidth().padding(Spacing.L.dp),
            verticalArrangement = Arrangement.spacedBy(Spacing.M.dp),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(DashboardSizes.rowGap)) {
                Text(
                    DashboardText.title(shown.widget.kind, shown.widget.groupBy),
                    Modifier.semantics { heading() },
                    color = MaterialTheme.colorScheme.onSurface,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    "${DashboardText.sourceName(shown.source)} · ${period(shown, state, dates)}",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
            val numbers = shown.numbers
            if (numbers == null) {
                Text(
                    DashboardText.notSent(shown.span),
                    Modifier.testTag("dashboard-not-sent-$id"),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            } else {
                when (shown.widget.kind) {
                    WidgetKind.TOTAL -> TotalNumbers(id, numbers)
                    WidgetKind.BARS_PER_DAY -> Bars(id, numbers, dates)
                    WidgetKind.TABLE -> Table(id, numbers, onOpen)
                    WidgetKind.DAY_SUMMARY -> Summary(id, numbers)
                }
                if (numbers.hiddenProjectCount > 0) {
                    Text(
                        DashboardText.hiddenProjects(numbers.hiddenProjectCount),
                        Modifier.testTag("dashboard-hidden-$id"),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }
        }
    }
}

/** The day, or the first and the last day, the numbers are for. */
private fun period(
    shown: WidgetState,
    state: DashboardState,
    dates: DashboardDates,
): String {
    val day = state.day
    val first = state.first
    val last = state.last
    return when {
        day == null || first == null || last == null -> ""
        shown.span == DashboardSpan.WEEK -> dates.range(first, last)
        else -> dates.text(day)
    }
}

@Composable
private fun TotalNumbers(
    id: String,
    numbers: DashboardNumbers,
) {
    Column(verticalArrangement = Arrangement.spacedBy(DashboardSizes.rowGap)) {
        Text(
            DashboardText.minutes(numbers.totalMinutes),
            Modifier.testTag("dashboard-value-$id"),
            color = MaterialTheme.colorScheme.onSurface,
            fontSize = DISPLAY_SIZE.sp,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            DashboardText.sessions(numbers.sessionCount),
            Modifier.testTag("dashboard-sessions-$id"),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun Bars(
    id: String,
    numbers: DashboardNumbers,
    dates: DashboardDates,
) {
    val tallest = numbers.days.maxOfOrNull { it.minutes } ?: 0
    val bar = chartColor(ColorRole.CHART_1)
    val none = MaterialTheme.colorScheme.surfaceVariant
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        numbers.days.forEach { day ->
            Column(
                Modifier
                    .weight(1f, fill = false)
                    .widthIn(max = BAR_WIDTH.dp)
                    .testTag("dashboard-bar-$id-${day.label}")
                    .clearAndSetSemantics {
                        contentDescription = "${dates.spoken(day.label)}, ${DashboardText.minutes(day.minutes)}"
                    },
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Spacing.XS.dp),
            ) {
                Box(Modifier.fillMaxWidth().height(CHART_HEIGHT.dp), contentAlignment = Alignment.BottomCenter) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(barHeight(day.minutes, tallest).dp)
                            .clip(MaterialTheme.shapes.small)
                            .background(if (day.minutes > 0) bar else none),
                    )
                }
                Text(
                    dates.weekday(day.label),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
    }
}

private fun barHeight(
    minutes: Int,
    tallest: Int,
): Float =
    if (minutes <= 0 || tallest <= 0) {
        BASELINE
    } else {
        maxOf(BASELINE, CHART_HEIGHT * minutes / tallest.toFloat())
    }

@Composable
private fun Table(
    id: String,
    numbers: DashboardNumbers,
    onOpen: (GroupMinutes) -> Unit,
) {
    if (numbers.groups.isEmpty()) {
        Text(
            DashboardText.NO_TABLE,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(Spacing.M.dp)) {
            numbers.groups.sortedByDescending { it.minutes }.forEach { GroupRow(id, it, onOpen) }
        }
    }
}

/** A group of a table: its name and time, its share of the whole as a bar and a percent, and its sessions. */
@Composable
private fun GroupRow(
    id: String,
    group: GroupMinutes,
    onOpen: (GroupMinutes) -> Unit,
) {
    val share = group.share.coerceIn(0.0, 1.0)
    val fill = chartColor(ColorRole.CHART_1)
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = "Show the sessions") { onOpen(group) }
            .testTag("dashboard-row-$id-${group.key}"),
        verticalArrangement = Arrangement.spacedBy(DashboardSizes.rowGap),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp)) {
            Text(
                group.key,
                Modifier.weight(1f),
                color = MaterialTheme.colorScheme.onSurface,
                style = MaterialTheme.typography.bodyLarge,
            )
            Text(
                DashboardText.minutes(group.minutes),
                color = MaterialTheme.colorScheme.onSurface,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.SemiBold,
            )
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        ) {
            Box(
                Modifier
                    .width(SHARE_WIDTH.dp)
                    .height(SHARE_HEIGHT.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            ) {
                Box(Modifier.width((SHARE_WIDTH * share).dp).height(SHARE_HEIGHT.dp).background(fill))
            }
            Text(
                "${(group.share * PERCENT).roundToInt()}% · ${DashboardText.sessions(group.sessions)}",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.labelMedium,
            )
        }
    }
}

/** The summary of a day: what the day came to, then one line for each project. */
@Composable
private fun Summary(
    id: String,
    numbers: DashboardNumbers,
) {
    if (numbers.groups.isEmpty()) {
        Text(
            DashboardText.NO_SUMMARY,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(Spacing.S.dp)) {
            Text(
                DashboardText.minutes(numbers.totalMinutes),
                Modifier.testTag("dashboard-value-$id"),
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = SECTION_SIZE.sp,
                fontWeight = FontWeight.SemiBold,
            )
            numbers.groups.sortedByDescending { it.minutes }.forEach { group ->
                Row(Modifier.fillMaxWidth().testTag("dashboard-line-$id-${group.key}")) {
                    Text(
                        group.key,
                        Modifier.weight(1f),
                        color = MaterialTheme.colorScheme.onSurface,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    Text(
                        DashboardText.minutes(group.minutes),
                        color = MaterialTheme.colorScheme.onSurface,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
            }
        }
    }
}

private const val DISPLAY_SIZE = 34
private const val SECTION_SIZE = 20
private const val CHART_HEIGHT = 120f
private const val BAR_WIDTH = 44
private const val BASELINE = 2f
private const val SHARE_WIDTH = 96
private const val SHARE_HEIGHT = 6
private const val PERCENT = 100
