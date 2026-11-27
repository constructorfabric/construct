package cf.construct.mobile.feature.dashboard

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.database.DatabaseConstruct
import cf.construct.mobile.core.database.SyncRecord
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.GroupBy
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.Outcome
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Position
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.Widget
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class DashboardHolderTest {
    /** The copy as the holder sees it: the document, and the numbers under the exact days they were counted for. */
    private class Copy(
        base: Construct,
        var document: DashboardDocument? = everyKind,
    ) : Construct by base {
        val held = mutableMapOf<DashboardInput, DashboardNumbers>()
        val asked = mutableListOf<DashboardInput>()
        var failure: String? = null
        private val changes = MutableSharedFlow<Unit>(replay = 1).also { it.tryEmit(Unit) }

        override val dashboardChanges: Flow<Unit> = changes

        fun changed() = changes.tryEmit(Unit)

        override suspend fun getDashboardDocument(): Outcome<DocumentState> = Outcome.Ok(DocumentState(document, null))

        override suspend fun getDashboardNumbers(input: DashboardInput): Outcome<DashboardNumbers> {
            asked += input
            return failure?.let { Outcome.Failed(it) }
                ?: held[input]?.let { Outcome.Ok(it) }
                ?: Outcome.Failed(DatabaseConstruct.NO_NUMBERS)
        }
    }

    private fun TestScope.copy(document: DashboardDocument? = everyKind) =
        Copy(FakeConstruct(backgroundScope), document)

    private fun TestScope.holder(
        copy: Construct,
        record: () -> SyncRecord = { SyncRecord(zone = "Europe/Berlin", syncedAt = SYNCED_AT) },
        now: Instant = WEDNESDAY_NOON,
    ) = holder(copy, record) { now }

    private fun TestScope.holder(
        copy: Construct,
        record: () -> SyncRecord = { SyncRecord(zone = "Europe/Berlin", syncedAt = SYNCED_AT) },
        now: () -> Instant,
    ) = DashboardHolder(copy, record, backgroundScope, now, UnconfinedTestDispatcher(testScheduler))

    /** Berlin's day of a date in November 2026 (UTC+1): from the evening of the day before. */
    private fun berlin(day: Int): DayBounds {
        fun two(number: Int) = number.toString().padStart(2, '0')
        return DayBounds("2026-11-${two(day)}", "2026-11-${two(day - 1)}T23:00:00Z", "2026-11-${two(day)}T23:00:00Z")
    }

    private fun forWidget(
        id: String,
        vararg days: DayBounds,
    ) = DashboardInput.ForWidget(days.toList(), id)

    private fun numbers(
        minutes: Int,
        vararg days: DayMinutes,
        hidden: Int = 0,
    ) = DashboardNumbers(
        totalMinutes = minutes,
        sessionCount = 3,
        days = days.toList(),
        groups = listOf(GroupMinutes("app", minutes - 35, 0.6, 2), GroupMinutes("web", 35, 0.4, 1)),
        hiddenProjectCount = hidden,
    )

    /** What the computer sends for every widget over the days of a span, with `minutes` for the total. */
    private fun Copy.hold(
        minutes: Int,
        vararg days: DayBounds,
    ) {
        document?.widgets?.forEach { widget ->
            // The summary counts one day, so it is only held for the one day of a span of one.
            if (widget.kind != WidgetKind.DAY_SUMMARY || days.size == 1) {
                val each = days.map { DayMinutes(it.label, minutes) }.toTypedArray()
                held[forWidget(widget.id, *days)] = numbers(minutes, *each)
            }
        }
    }

    @Test
    fun drawsEveryWidgetKindWithTheNumbersOfTodayAndTellsWhenTheyWereUpdated() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()

            val state = holder.state.value
            assertFalse(state.loading)
            assertTrue(state.hasDashboard)
            assertEquals("My day", state.title)
            assertEquals(DashboardSpan.DAY, state.span)
            assertEquals(CalendarDay(2026, 11, 18), state.day)
            assertEquals(CalendarDay(2026, 11, 18), state.today)
            assertEquals("Europe/Berlin", state.zoneName)
            assertEquals(SYNCED_AT, state.updatedAt)
            assertEquals(
                listOf(WidgetKind.TOTAL, WidgetKind.BARS_PER_DAY, WidgetKind.TABLE, WidgetKind.DAY_SUMMARY),
                state.widgets.map { it.widget.kind },
            )
            assertTrue(state.widgets.all { it.numbers?.totalMinutes == 95 })
            assertFalse(state.nothingSent)
        }

    @Test
    fun namesTheDaysAsTheComputerDoesInItsTimeZone() =
        runTest {
            val copy = copy()
            val holder = holder(copy)
            runCurrent()

            assertEquals(
                listOf(DayBounds("2026-11-18", "2026-11-17T23:00:00Z", "2026-11-18T23:00:00Z")),
                (copy.asked.first() as DashboardInput.ForWidget).days,
            )
            // A day with a clock change is 25 hours long, and starts when the day before ends.
            val autumn = holder(copy, now = Instant.parse("2026-10-25T12:00:00Z"))
            runCurrent()
            assertEquals(
                DayBounds("2026-10-25", "2026-10-24T22:00:00Z", "2026-10-25T23:00:00Z"),
                (copy.asked.last() as DashboardInput.ForWidget).days.single(),
            )
            assertEquals(CalendarDay(2026, 10, 25), autumn.state.value.day)
        }

    @Test
    fun movingToThePreviousDayAndWeekShowsTheNumbersOfThatDayAndWeek() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            copy.hold(60, berlin(17))
            copy.hold(300, *(16..22).map(::berlin).toTypedArray())
            copy.hold(420, *(9..15).map(::berlin).toTypedArray())
            val holder = holder(copy)
            runCurrent()

            holder.previous()
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 17), holder.state.value.day)
            assertTrue(
                holder.state.value.widgets
                    .all { it.numbers?.totalMinutes == 60 },
            )
            assertFalse(holder.state.value.onToday)

            holder.showSpan(DashboardSpan.WEEK)
            runCurrent()
            val week = holder.state.value
            assertEquals(CalendarDay(2026, 11, 16), week.first)
            assertEquals(CalendarDay(2026, 11, 22), week.last)
            assertEquals(CalendarDay(2026, 11, 17), week.day)
            assertEquals(
                listOf(WidgetKind.TOTAL, WidgetKind.BARS_PER_DAY, WidgetKind.TABLE),
                week.widgets.filter { it.numbers?.totalMinutes == 300 }.map { it.widget.kind },
            )
            assertEquals(DashboardSpan.WEEK, week.widgets.first().span)

            holder.previous()
            runCurrent()
            val before = holder.state.value
            assertEquals(CalendarDay(2026, 11, 10), before.day)
            assertEquals(CalendarDay(2026, 11, 9), before.first)
            assertEquals(CalendarDay(2026, 11, 15), before.last)
            assertEquals(
                420,
                before.widgets
                    .first()
                    .numbers
                    ?.totalMinutes,
            )
        }

    /** The page draws the widgets of [source] and no others, and the switch shows [source] as chosen. */
    private fun DashboardHolder.assertDrawn(source: Source) {
        assertEquals(source, state.value.source)
        assertTrue(state.value.widgets.isNotEmpty())
        assertTrue(state.value.widgets.all { it.source == source })
    }

    @Test
    fun theSwitchChangesTheWidgetsDrawnAndSurvivesMovingByDayByWeekAndToADate() =
        runTest {
            val copy = copy(bothSources)
            copy.hold(95, berlin(18))
            copy.hold(60, berlin(17))
            copy.hold(300, *(16..22).map(::berlin).toTypedArray())
            copy.hold(40, berlin(12))
            val holder = holder(copy)
            runCurrent()

            val sessions = holder.state.value
            assertTrue(sessions.canSwitchSource)
            assertEquals(listOf(Source.AI_SESSIONS, Source.ON_SCREEN), sessions.sources)
            assertEquals(listOf("total", "bars"), sessions.widgets.map { it.widget.id })
            holder.assertDrawn(Source.AI_SESSIONS)

            holder.showSource(Source.ON_SCREEN)
            runCurrent()
            holder.assertDrawn(Source.ON_SCREEN)
            assertEquals(
                listOf("apps", "summary"),
                holder.state.value.widgets
                    .map { it.widget.id },
            )

            holder.previous()
            runCurrent()
            holder.assertDrawn(Source.ON_SCREEN)
            assertEquals(CalendarDay(2026, 11, 17), holder.state.value.day)

            holder.showSpan(DashboardSpan.WEEK)
            runCurrent()
            holder.assertDrawn(Source.ON_SCREEN)
            assertEquals(CalendarDay(2026, 11, 16), holder.state.value.first)
            assertEquals(
                300,
                holder.state.value.widgets
                    .first()
                    .numbers
                    ?.totalMinutes,
            )

            holder.pick(CalendarDay(2026, 11, 12))
            runCurrent()
            holder.assertDrawn(Source.ON_SCREEN)
            assertEquals(CalendarDay(2026, 11, 12), holder.state.value.day)
        }

    @Test
    fun theSwitchGoesBackToTheWidgetsOfTheFirstSource() =
        runTest {
            val holder = holder(copy(bothSources))
            holder.showSource(Source.ON_SCREEN)
            runCurrent()
            holder.showSource(Source.AI_SESSIONS)
            runCurrent()
            assertEquals(
                listOf("total", "bars"),
                holder.state.value.widgets
                    .map { it.widget.id },
            )
        }

    @Test
    fun theWidgetsOfTheOtherSourceAreNotSentWhenTheCopyHoldsNoNumbersForThem() =
        runTest {
            val copy = copy(bothSources)
            copy.held[forWidget("total", berlin(18))] = numbers(95, DayMinutes("2026-11-18", 95))
            val holder = holder(copy)
            runCurrent()
            assertFalse(holder.state.value.nothingSent)

            holder.showSource(Source.ON_SCREEN)
            runCurrent()

            val state = holder.state.value
            assertNull(state.failure)
            assertTrue(state.widgets.all { it.numbers == null })
            assertTrue(state.nothingSent)
        }

    @Test
    fun theSwitchIsHiddenForADocumentOfOneSource() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()

            assertFalse(holder.state.value.canSwitchSource)
            assertEquals(listOf(Source.AI_SESSIONS), holder.state.value.sources)
            assertEquals(4, holder.state.value.widgets.size)

            // A document that counts the screen only draws it, and shows no switch either.
            copy.document = everyKind.copy(source = Source.ON_SCREEN)
            copy.changed()
            runCurrent()
            assertFalse(holder.state.value.canSwitchSource)
            assertEquals(Source.ON_SCREEN, holder.state.value.source)
            assertEquals(4, holder.state.value.widgets.size)
        }

    @Test
    fun aChoiceTheDocumentNoLongerOffersFallsBackToItsFirstSource() =
        runTest {
            val copy = copy(bothSources)
            val holder = holder(copy)
            holder.showSource(Source.ON_SCREEN)
            runCurrent()
            assertEquals(Source.ON_SCREEN, holder.state.value.source)

            copy.document = everyKind
            copy.changed()
            runCurrent()

            assertEquals(Source.AI_SESSIONS, holder.state.value.source)
            assertEquals(4, holder.state.value.widgets.size)

            // The choice is kept: when the document offers the source again, the page returns to it.
            copy.document = bothSources
            copy.changed()
            runCurrent()
            assertEquals(Source.ON_SCREEN, holder.state.value.source)
        }

    @Test
    fun theSummaryOfADayCountsOneDayWhenThePageCountsAWeek() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            holder.showSpan(DashboardSpan.WEEK)
            runCurrent()

            val summary =
                holder.state.value.widgets
                    .single { it.widget.kind == WidgetKind.DAY_SUMMARY }
            assertEquals(DashboardSpan.DAY, summary.span)
            assertEquals(95, summary.numbers?.totalMinutes)
        }

    @Test
    fun pickingADateShowsTheNumbersOfThatDate() =
        runTest {
            val copy = copy()
            copy.hold(40, berlin(12))
            val holder = holder(copy)
            runCurrent()

            holder.pick(CalendarDay(2026, 11, 12))
            runCurrent()

            assertEquals(CalendarDay(2026, 11, 12), holder.state.value.day)
            assertTrue(
                holder.state.value.widgets
                    .all { it.numbers?.totalMinutes == 40 },
            )
            // A date that does not exist changes nothing, and a date after today is today.
            holder.pick(CalendarDay(2026, 2, 30))
            holder.pick(CalendarDay(2027, 1, 5))
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.day)
            assertTrue(holder.state.value.onToday)
        }

    @Test
    fun aDayTheCopyHoldsNothingForIsNotSentAndNotZeros() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()

            holder.previous()
            runCurrent()

            val state = holder.state.value
            assertNull(state.failure)
            assertTrue(state.widgets.all { it.numbers == null })
            assertTrue(state.nothingSent)
        }

    @Test
    fun widgetsWithoutNumbersAreTheOnlyOnesLeftOut() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            copy.held.remove(forWidget("table", berlin(18)))
            val holder = holder(copy)
            runCurrent()

            val state = holder.state.value
            assertFalse(state.nothingSent)
            assertNull(state.widgets.single { it.widget.kind == WidgetKind.TABLE }.numbers)
            assertNotNull(state.widgets.single { it.widget.kind == WidgetKind.TOTAL }.numbers)
        }

    @Test
    fun theProjectsHiddenOnTheComputerReachTheWidget() =
        runTest {
            val copy = copy()
            copy.held[forWidget("table", berlin(18))] = numbers(95, DayMinutes("2026-11-18", 95), hidden = 2)
            val holder = holder(copy)
            runCurrent()

            val table =
                holder.state.value.widgets
                    .single { it.widget.kind == WidgetKind.TABLE }
            assertEquals(2, table.numbers?.hiddenProjectCount)
        }

    @Test
    fun anEmptyCopyHasNoDashboardAndAnEraseEmptiesThePage() =
        runTest {
            val copy = copy(document = null)
            val holder = holder(copy)
            runCurrent()

            assertTrue(holder.state.value.isEmpty)
            assertTrue(
                holder.state.value.widgets
                    .isEmpty(),
            )

            copy.document = everyKind
            copy.hold(95, berlin(18))
            copy.changed()
            runCurrent()
            assertFalse(holder.state.value.isEmpty)
            assertEquals(4, holder.state.value.widgets.size)

            // "Delete everything" on the computer empties the copy at the next sync.
            copy.document = null
            copy.held.clear()
            copy.changed()
            runCurrent()
            assertTrue(holder.state.value.isEmpty)
        }

    @Test
    fun aSyncThatChangesTheNumbersShowsWithoutARefresh() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()

            copy.hold(120, berlin(18))
            copy.changed()
            runCurrent()

            assertTrue(
                holder.state.value.widgets
                    .all { it.numbers?.totalMinutes == 120 },
            )
        }

    @Test
    fun nothingIsCountedForTheFuture() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()
            assertFalse(holder.state.value.canGoNext)

            holder.next()
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.day)

            holder.previous()
            runCurrent()
            assertTrue(holder.state.value.canGoNext)
            holder.next()
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.day)

            // The week of today can be reached from the week before, and the next one cannot.
            holder.showSpan(DashboardSpan.WEEK)
            runCurrent()
            holder.previous()
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 11), holder.state.value.day)
            assertTrue(holder.state.value.canGoNext)
            holder.next()
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.day)
            assertFalse(holder.state.value.canGoNext)
        }

    @Test
    fun todayBringsThePageBackAndFollowsTheClock() =
        runTest {
            val copy = copy()
            val holder = holder(copy)
            runCurrent()
            holder.previous()
            runCurrent()
            assertFalse(holder.state.value.onToday)

            holder.showToday()
            runCurrent()

            assertTrue(holder.state.value.onToday)
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.day)
        }

    @Test
    fun aCopyThatCannotBeReadIsAFailureNotAnEmptyPage() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            val holder = holder(copy)
            runCurrent()

            copy.failure = DatabaseConstruct.UNREADABLE
            copy.changed()
            runCurrent()

            val state = holder.state.value
            assertEquals(DatabaseConstruct.UNREADABLE, state.failure)
            assertFalse(state.isEmpty)
            assertFalse(state.loading)

            copy.failure = null
            holder.refresh()
            runCurrent()
            assertNull(holder.state.value.failure)
        }

    @Test
    fun aRecordThatCannotBeReadIsAFailure() =
        runTest {
            val holder = holder(copy(), record = { error("unreadable") })
            runCurrent()

            assertEquals("the copy could not be read", holder.state.value.failure)
        }

    @Test
    fun aZoneThePhoneDoesNotKnowIsNotAFailure() =
        runTest {
            val holder = holder(copy(), record = { SyncRecord(zone = "Nowhere/Land") })
            runCurrent()

            val state = holder.state.value
            assertNull(state.failure)
            assertTrue(state.hasDashboard)
            assertTrue(state.nothingSent)
        }

    @Test
    fun todayMovesWhenTheClockPassesMidnightWhileThePageIsOpen() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(18))
            copy.hold(40, berlin(19))
            // 23:30 in Berlin on the 18th; the clock runs with the test's own time.
            val start = Instant.parse("2026-11-18T22:30:00Z")
            val holder = holder(copy) { start + testScheduler.currentTime.milliseconds }
            runCurrent()
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.today)
            assertEquals(
                95,
                holder.state.value.widgets
                    .first()
                    .numbers
                    ?.totalMinutes,
            )

            advanceTimeBy(29.minutes)
            assertEquals(CalendarDay(2026, 11, 18), holder.state.value.today)

            advanceTimeBy(2.minutes)
            val state = holder.state.value
            assertEquals(CalendarDay(2026, 11, 19), state.today)
            assertEquals(CalendarDay(2026, 11, 19), state.day)
            assertTrue(state.onToday)
            assertEquals(
                40,
                state.widgets
                    .first()
                    .numbers
                    ?.totalMinutes,
            )
        }

    @Test
    fun aDayThePersonPickedStaysWhenTheClockPassesMidnight() =
        runTest {
            val copy = copy()
            copy.hold(95, berlin(17))
            val start = Instant.parse("2026-11-18T22:30:00Z")
            val holder = holder(copy) { start + testScheduler.currentTime.milliseconds }
            runCurrent()
            holder.pick(CalendarDay(2026, 11, 17))
            runCurrent()

            advanceTimeBy(31.minutes)
            val state = holder.state.value
            assertEquals(CalendarDay(2026, 11, 19), state.today)
            assertEquals(CalendarDay(2026, 11, 17), state.day)
            assertFalse(state.onToday)
            assertEquals(
                95,
                state.widgets
                    .first()
                    .numbers
                    ?.totalMinutes,
            )
        }

    private companion object {
        const val SYNCED_AT = "2026-11-18T10:29:00Z"

        /** Wednesday 18 November 2026, 11:30 in Berlin. */
        val WEDNESDAY_NOON: Instant = Instant.parse("2026-11-18T10:30:00Z")

        /** The document of the computer with widgets of both sources: two count AI sessions, two the screen. */
        val bothSources =
            DashboardDocument(
                title = "My day",
                period = Period.TODAY,
                sessions = Whose.ALL,
                widgets =
                    listOf(
                        Widget("total", WidgetKind.TOTAL, Position(0, 0, 4, 4)),
                        Widget("bars", WidgetKind.BARS_PER_DAY, Position(4, 0, 8, 4)),
                        Widget(
                            "apps",
                            WidgetKind.TABLE,
                            Position(0, 4, 12, 3),
                            groupBy = GroupBy.APP,
                            source = Source.ON_SCREEN,
                        ),
                        Widget("summary", WidgetKind.DAY_SUMMARY, Position(0, 7, 12, 3), source = Source.ON_SCREEN),
                    ),
            )

        val everyKind =
            DashboardDocument(
                title = "My day",
                period = Period.TODAY,
                sessions = Whose.ALL,
                widgets =
                    listOf(
                        // Listed out of order: the page follows the grid.
                        Widget("summary", WidgetKind.DAY_SUMMARY, Position(0, 7, 12, 3)),
                        Widget("table", WidgetKind.TABLE, Position(0, 4, 12, 3), groupBy = GroupBy.PROJECT),
                        Widget("bars", WidgetKind.BARS_PER_DAY, Position(4, 0, 8, 4)),
                        Widget("total", WidgetKind.TOTAL, Position(0, 0, 4, 4)),
                    ),
            )
    }
}
