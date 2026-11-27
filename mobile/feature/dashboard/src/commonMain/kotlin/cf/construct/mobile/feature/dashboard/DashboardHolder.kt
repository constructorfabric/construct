package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.DatabaseConstruct
import cf.construct.mobile.core.database.SyncRecord
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.Widget
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.datetime.DateTimeUnit
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.isoDayNumber
import kotlinx.datetime.number
import kotlinx.datetime.plus
import kotlinx.datetime.toLocalDateTime
import kotlin.time.Clock
import kotlin.time.Instant

/**
 * Turns the dashboard the phone holds into one [DashboardState]: the computer's document, each widget with the numbers
 * the copy has for the day or the week the person is on, and the movement between days and weeks. It reads only
 * [construct], which is the phone's copy, and never asks the computer, so it works with the computer off. It is plain
 * common Kotlin with no Android type.
 *
 * The phone counts nothing. The computer sends the numbers of each widget for one day and for each week (Monday to
 * Sunday) that has activity, and the copy keeps each under the exact days it was counted for. So the holder names the
 * days as the computer does (its time zone, from [record]) and asks the copy for those. A day or a week the copy has
 * no entry for is `null` numbers in [WidgetState], not zeros: the computer sent nothing for it.
 *
 * The copy is watched, so a sync shows by itself. Call [refresh] and run [scope] on one thread, the main one, as the
 * other holders: a refresh starts the watch again and drops the one before it.
 *
 * @param record what the copy remembers: the computer's time zone and when the copy was last brought up to date.
 * @param now the time, for "today".
 * @param storage where [record] is read.
 */
@Suppress("TooManyFunctions")
class DashboardHolder(
    private val construct: Construct,
    private val record: () -> SyncRecord,
    private val scope: CoroutineScope,
    private val now: () -> Instant,
    private val storage: CoroutineDispatcher,
) {
    /** For Swift, which sees no default arguments: the clock of the phone, and the default dispatcher for storage. */
    constructor(
        construct: Construct,
        record: () -> SyncRecord,
        scope: CoroutineScope,
    ) : this(construct, record, scope, { Clock.System.now() }, Dispatchers.Default)

    /**
     * What the person chose: how long a stretch, which day (none: today, whenever today is), and what the widgets count
     * (none: the first source the document has). A chosen source the document stops offering is not drawn, but it is
     * kept: when the document offers it again, the page returns to it.
     */
    private data class Choice(
        val span: DashboardSpan = DashboardSpan.DAY,
        val day: LocalDate? = null,
        val source: Source? = null,
    )

    private val choice = MutableStateFlow(Choice())
    private val mutableState = MutableStateFlow(DashboardState())
    val state: StateFlow<DashboardState> = mutableState.asStateFlow()
    private var watching: Job? = null

    init {
        refresh()
    }

    /**
     * Starts the watch again, to read at once. What is on screen stays while it runs, and a watch still running is
     * dropped.
     */
    fun refresh() {
        watching?.cancel()
        mutableState.update { it.copy(loading = true, failure = null) }
        watching =
            scope.launch {
                combine(choice, construct.dashboardChanges) { chosen, _ -> chosen }.collectLatest { chosen ->
                    // "Today" follows the clock: read again when the computer's day ends, whatever the page shows.
                    while (true) {
                        delay(show(chosen))
                    }
                }
            }
    }

    /** Counts days or weeks from now on, on the same day. */
    fun showSpan(span: DashboardSpan) = choice.update { it.copy(span = span) }

    /** Draws the widgets that count [source]. Moving by day or week, or to a date, keeps it. */
    fun showSource(source: Source) = choice.update { it.copy(source = source) }

    /** One day back, or one week back when the page counts weeks. No limit: the copy may hold nothing there. */
    fun previous() = move(-1)

    /** One day on, or one week on; nothing when [DashboardState.canGoNext] is false. */
    fun next() = move(1)

    /** Back to today, and staying on today when the day changes. */
    fun showToday() = choice.update { it.copy(day = null) }

    /** Stands on a date of the computer's calendar. A date that does not exist is ignored; one after today is today. */
    fun pick(day: CalendarDay) {
        val date = day.toDateOrNull() ?: return
        val today = mutableState.value.today?.toDateOrNull()
        choice.update { it.copy(day = if (today != null && date > today) today else date) }
    }

    private fun move(direction: Int) {
        val today = mutableState.value.today?.toDateOrNull() ?: return
        choice.update { chosen ->
            val from = minOf(chosen.day ?: today, today)
            val to = from.plus(direction * length(chosen.span), DateTimeUnit.DAY)
            if (first(chosen.span, to) > today) chosen else chosen.copy(day = minOf(to, today))
        }
    }

    /** Reads the copy and shows it. Returns how long to wait, in milliseconds, before today may have changed. */
    private suspend fun show(chosen: Choice): Long {
        val read = construct.getDashboardDocument()
        val remembered = remembered(record, storage)
        when {
            read is Outcome.Failed -> mutableState.fail(read.error)
            remembered == null -> mutableState.fail(UNREADABLE)
            read is Outcome.Ok -> showPage(chosen, read.value.document, remembered)
        }
        return untilMidnight(zoneOf(remembered?.zone))
    }

    /** The time to the start of the next day of [zone], at least a second so that a failing read is not a busy loop. */
    private fun untilMidnight(zone: TimeZone): Long {
        val moment = now()
        val next =
            moment
                .toLocalDateTime(zone)
                .date
                .plus(1, DateTimeUnit.DAY)
                .atStartOfDayIn(zone)
        return (next - moment).inWholeMilliseconds.coerceAtLeast(MIN_WAIT_MILLIS)
    }

    private suspend fun showPage(
        chosen: Choice,
        document: DashboardDocument?,
        remembered: SyncRecord,
    ) {
        val zone = zoneOf(remembered.zone)
        val today = now().toLocalDateTime(zone).date
        val day = chosen.day?.let { minOf(it, today) } ?: today
        val sources = document?.let(::sourcesOf).orEmpty()
        // A source the document no longer has (it changed on the computer) falls back to the first one it has.
        val source = chosen.source?.takeIf { it in sources } ?: sources.firstOrNull() ?: Source.AI_SESSIONS
        val read = document?.let { widgetsOf(it, source, chosen.span, day, zone) } ?: Outcome.Ok(emptyList())
        when (val widgets = read) {
            is Outcome.Failed -> {
                mutableState.fail(widgets.error)
            }

            is Outcome.Ok -> {
                mutableState.value =
                    DashboardState(
                        loading = false,
                        hasDashboard = document != null,
                        title = document?.title.orEmpty(),
                        span = chosen.span,
                        source = source,
                        sources = sources,
                        day = day.toCalendarDay(),
                        first = first(chosen.span, day).toCalendarDay(),
                        last = last(chosen.span, day).toCalendarDay(),
                        today = today.toCalendarDay(),
                        zoneName = remembered.zone,
                        onToday = last(chosen.span, day) >= today,
                        canGoNext = first(chosen.span, day.plus(length(chosen.span), DateTimeUnit.DAY)) <= today,
                        updatedAt = remembered.syncedAt,
                        widgets = widgets.value,
                    )
            }
        }
    }

    /**
     * The widgets that count [source] with their numbers, or why a read failed. A day the copy holds no numbers for
     * gives a widget with null numbers.
     */
    private suspend fun widgetsOf(
        document: DashboardDocument,
        source: Source,
        span: DashboardSpan,
        day: LocalDate,
        zone: TimeZone,
    ): Outcome<List<WidgetState>> {
        val widgets = mutableListOf<WidgetState>()
        val drawn = document.widgets.filter { sourceOf(it, document) == source }
        for (widget in drawn.sortedWith(compareBy({ it.position.row }, { it.position.column }))) {
            // The summary of a day counts one day, whatever the page counts.
            val widgetSpan = if (widget.kind == WidgetKind.DAY_SUMMARY) DashboardSpan.DAY else span
            val input = DashboardInput.ForWidget(daysOf(widgetSpan, day, zone), widget.id)
            val numbers =
                when (val read = construct.getDashboardNumbers(input)) {
                    is Outcome.Ok -> {
                        read.value
                    }

                    is Outcome.Failed -> {
                        if (read.error != DatabaseConstruct.NO_NUMBERS) return read
                        null
                    }
                }
            widgets += WidgetState(widget, widgetSpan, numbers, sourceOf(widget, document))
        }
        return Outcome.Ok(widgets)
    }

    private companion object {
        const val UNREADABLE = "the copy could not be read"
        const val DAYS_PER_WEEK = 7
        const val MIN_WAIT_MILLIS = 1_000L

        /** What a widget counts the time of: its own source, the document's, or the AI sessions. */
        fun sourceOf(
            widget: Widget,
            document: DashboardDocument,
        ): Source = widget.source ?: document.source ?: Source.AI_SESSIONS

        /** The sources the widgets of the document count, in the order of [Source]. */
        fun sourcesOf(document: DashboardDocument): List<Source> =
            document.widgets
                .map { sourceOf(it, document) }
                .distinct()
                .sorted()

        fun length(span: DashboardSpan) = if (span == DashboardSpan.WEEK) DAYS_PER_WEEK else 1

        /** The first day of what the page counts: the day, or the Monday of its week. */
        fun first(
            span: DashboardSpan,
            day: LocalDate,
        ): LocalDate =
            if (span == DashboardSpan.WEEK) day.plus(-(day.dayOfWeek.isoDayNumber - 1), DateTimeUnit.DAY) else day

        fun last(
            span: DashboardSpan,
            day: LocalDate,
        ): LocalDate = first(span, day).plus(length(span) - 1, DateTimeUnit.DAY)

        /** A zone the phone does not know is the phone's own: its days will not match, so nothing is found. */
        fun zoneOf(name: String?): TimeZone =
            name?.let { runCatching { TimeZone.of(it) }.getOrNull() } ?: TimeZone.currentSystemDefault()

        /** The days of a span as the computer wrote them into the input: label, and start and end in UTC minutes. */
        fun daysOf(
            span: DashboardSpan,
            day: LocalDate,
            zone: TimeZone,
        ): List<DayBounds> =
            (0 until length(span)).map { offset ->
                val date = first(span, day).plus(offset, DateTimeUnit.DAY)
                DayBounds(
                    label = date.toString(),
                    start = utcMinute(date.atStartOfDayIn(zone)),
                    end = utcMinute(date.plus(1, DateTimeUnit.DAY).atStartOfDayIn(zone)),
                )
            }

        /** A moment the way the computer writes it: UTC, a whole minute, `2026-11-17T23:00:00Z`. */
        fun utcMinute(moment: Instant): String {
            val utc = moment.toLocalDateTime(TimeZone.UTC)
            return "${utc.date}T${utc.hour.twoDigits()}:${utc.minute.twoDigits()}:00Z"
        }

        fun Int.twoDigits() = toString().padStart(2, '0')

        fun LocalDate.toCalendarDay() = CalendarDay(year, month.number, day)
    }
}

private fun CalendarDay.toDateOrNull(): LocalDate? = runCatching { LocalDate(year, month, day) }.getOrNull()

/** What is on screen stays; the reason of the failed read shows beside it. */
private fun MutableStateFlow<DashboardState>.fail(reason: String) =
    update { it.copy(loading = false, failure = reason) }

/** What the copy remembers, or null when it cannot be read (a damaged file, a full disk). */
@Suppress("TooGenericExceptionCaught")
private suspend fun remembered(
    record: () -> SyncRecord,
    storage: CoroutineDispatcher,
): SyncRecord? =
    try {
        withContext(storage) { record() }
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (_: Exception) {
        null
    }
