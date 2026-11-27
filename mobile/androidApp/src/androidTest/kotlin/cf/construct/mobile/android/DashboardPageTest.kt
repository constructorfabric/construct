package cf.construct.mobile.android

import android.content.Context
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.CopyUpdate
import cf.construct.mobile.core.database.DashboardUpdate
import cf.construct.mobile.core.database.PhoneConstruct
import cf.construct.mobile.core.database.androidCopyDriver
import cf.construct.mobile.core.keys.FakeModelSettingsStore
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.GroupBy
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Position
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.Widget
import cf.construct.mobile.core.model.WidgetKind
import cf.construct.mobile.feature.dashboard.DashboardActions
import cf.construct.mobile.feature.dashboard.DashboardHolder
import cf.construct.mobile.feature.dashboard.DashboardScreen
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Rule
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test

private const val WAIT_MS = 10_000L
private const val ZONE = "Europe/Berlin"

/**
 * The Dashboard page on the phone's own copy, written the way a sync writes it. Nothing here connects to a computer, so
 * every number shown is one the copy holds, which is also what a computer that is switched off leaves.
 */
class DashboardPageTest {
    @get:Rule
    val compose = createComposeRule()

    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext
    private val file = "dashboard-page-test.db"
    private lateinit var store: CopyStore
    private lateinit var scope: CoroutineScope
    private val zone = ZoneId.of(ZONE)
    private val today: LocalDate = LocalDate.now(zone)
    private val monday: LocalDate = today.with(DayOfWeek.MONDAY)

    @BeforeTest
    fun open() {
        context.deleteDatabase(file)
        store = CopyStore(androidCopyDriver(context, file))
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    }

    @AfterTest
    fun close() {
        scope.cancel()
        context.deleteDatabase(file)
    }

    @Test
    fun drawsEveryWidgetKindWithTheNumbersOfTodayAndWhenTheyWereUpdated() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        for (widget in listOf("total", "per-day", "by-project", "end-of-day")) {
            compose.waitUntil(WAIT_MS) { has("dashboard-widget-$widget") }
        }
        compose.onNodeWithTag("dashboard-value-total").assertTextIs("1 h 35 min")
        compose.onNodeWithTag("dashboard-sessions-total").assertTextIs("3 sessions")
        compose.onNodeWithTag("dashboard-bar-per-day-$today").assertExists()
        compose.onNodeWithTag("dashboard-row-by-project-app").assertExists()
        compose.onNodeWithTag("dashboard-line-end-of-day-app").assertExists()
        compose.onNodeWithTag("dashboard-updated").assertTextStartsWith("Updated ")
        compose.onAllNodesWithTag("dashboard-hidden-total").assertCountEquals(0)
    }

    @Test
    fun showsThePreviousDayThePreviousWeekAndAPickedDate() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }

        compose.onNodeWithTag("dashboard-previous").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-today") }
        compose.onNodeWithTag("dashboard-value-total").assertTextIs("30 min")

        // The page counts weeks: this week holds today and yesterday, unless yesterday is in the week before.
        compose.onNodeWithTag("dashboard-today").performClick()
        compose.onNodeWithText("Week").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") || has("dashboard-not-sent") }
        compose.onNodeWithTag("dashboard-previous").performClick()
        // The week before holds no numbers: the page says so and shows no zero.
        compose.waitUntil(WAIT_MS) { has("dashboard-not-sent") }
        compose.onAllNodesWithTag("dashboard-value-total").assertCountEquals(0)
        compose
            .onNodeWithTag("dashboard-not-sent")
            .assertTextIs("Your computer has not sent the numbers for this week.")
    }

    @Test
    fun aPickedDateShowsTheNumbersOfThatDay() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        val picked = monday.minusDays(5)
        compose.onNodeWithTag("dashboard-date").performClick()
        compose.onNodeWithTag("dashboard-calendar").assertExists()
        // The calendar opens on today's month; the day of the earlier week may be in the month before.
        if (picked.month != today.month) compose.onNodeWithText("Previous month", substring = true).performClick()
        compose.onNode(hasText(picked.dayOfMonth.toString()) and hasClickAction()).performClick()
        compose.onNodeWithTag("dashboard-calendar-done").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        compose.onNodeWithTag("dashboard-value-total").assertTextIs("50 min")
    }

    @Test
    fun aDayTheCopyHoldsNothingForSaysSoAndShowsNoZeros() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        // Three days back is a day the copy holds no numbers for.
        repeat(3) { compose.onNodeWithTag("dashboard-previous").performClick() }
        compose.waitUntil(WAIT_MS) { has("dashboard-not-sent") }
        compose.onAllNodesWithTag("dashboard-value-total").assertCountEquals(0)
        compose
            .onNodeWithTag("dashboard-not-sent")
            .assertTextIs("Your computer has not sent the numbers for this day.")
        compose.onNodeWithTag("dashboard-next").assertExists()
    }

    @Test
    fun theSwitchChangesTheWidgetsAndIsNotShownForADocumentOfOneSource() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        compose.onAllNodesWithTag("dashboard-source").assertCountEquals(0)
        store.deleteEverything()
        compose.waitUntil(WAIT_MS) { has("dashboard-empty") }

        copyOf(todayMinutes = 95, hidden = 0, onScreen = true)
        compose.waitUntil(WAIT_MS) { has("dashboard-source") }
        compose.onNodeWithTag("dashboard-value-total").assertTextIs("1 h 35 min")
        compose.onAllNodesWithTag("dashboard-widget-by-app").assertCountEquals(0)

        compose.onNodeWithTag("dashboard-source-on-screen").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-widget-by-app") }
        compose.onAllNodesWithTag("dashboard-widget-total").assertCountEquals(0)

        // The choice stays when the page moves a day: the copy holds no numbers for the on-screen table on yesterday.
        compose.onNodeWithTag("dashboard-previous").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-not-sent") }
        compose.onNodeWithTag("dashboard-source-on-screen").assertExists()
        compose.onNodeWithTag("dashboard-source-ai-sessions").performClick()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
    }

    @Test
    fun theNextDayCannotBeReachedFromToday() {
        copyOf(todayMinutes = 95, hidden = 0)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        compose.onNodeWithTag("dashboard-next").assertIsNotEnabled()
    }

    @Test
    fun showsTheHiddenProjectsLineWhenTheComputerHidesSome() {
        copyOf(todayMinutes = 95, hidden = 2)
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-hidden-total") }
        compose.onNodeWithTag("dashboard-hidden-total").assertTextIs("2 projects are hidden on your computer.")
    }

    @Test
    fun showsTheEmptyStateWithNothingInTheCopyAndAfterDeleteEverything() {
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-empty") }
        compose.onNodeWithText("Nothing from your computer yet").assertExists()
        copyOf(todayMinutes = 95, hidden = 0)
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        store.deleteEverything()
        compose.waitUntil(WAIT_MS) { has("dashboard-empty") }
        compose.onAllNodesWithTag("dashboard-widget-total").assertCountEquals(0)
    }

    @Test
    fun theNumbersStayWithTheComputerOff() {
        copyOf(todayMinutes = 95, hidden = 0)
        // The page is built on the copy alone: there is no connection in this test for it to ask.
        show()
        compose.waitUntil(WAIT_MS) { has("dashboard-value-total") }
        compose.onNodeWithTag("dashboard-value-total").assertTextIs("1 h 35 min")
        compose.onNodeWithTag("dashboard-updated").assertTextStartsWith("Updated ")
    }

    private fun show() {
        val construct = PhoneConstruct(store, FakeModelSettingsStore(), Dispatchers.IO)
        val holder = DashboardHolder(construct, store::syncRecord, scope)
        val actions =
            DashboardActions(
                holder::showSpan,
                holder::showSource,
                holder::previous,
                holder::next,
                holder::showToday,
                holder::pick,
            )
        compose.setContent {
            ConstructTheme {
                val state by holder.state.collectAsState()
                DashboardScreen(state, actions)
            }
        }
    }

    private fun has(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    /**
     * What a sync writes: the computer's dashboard, and the numbers it counted for today (`todayMinutes`), for
     * yesterday (30 min), for the week of today and for a day of the week before (50 min); nothing for other days.
     */
    private fun copyOf(
        todayMinutes: Int,
        hidden: Int,
        onScreen: Boolean = false,
    ) {
        val widgets =
            listOfNotNull(
                Widget("total", WidgetKind.TOTAL, Position(0, 0, 12, 2)),
                Widget("per-day", WidgetKind.BARS_PER_DAY, Position(0, 2, 12, 4)),
                Widget("by-project", WidgetKind.TABLE, Position(0, 6, 12, 5), groupBy = GroupBy.PROJECT),
                Widget("end-of-day", WidgetKind.DAY_SUMMARY, Position(0, 11, 12, 3)),
                Widget(
                    "by-app",
                    WidgetKind.TABLE,
                    Position(0, 14, 12, 5),
                    groupBy = GroupBy.APP,
                    source = Source.ON_SCREEN,
                ).takeIf { onScreen },
            )
        val document = DashboardDocument("Activity", Period.THIS_WEEK, Whose.OWN, widgets)
        val picked = monday.minusDays(5)
        val numbers = mutableListOf<Pair<DashboardInput, DashboardNumbers>>()

        fun add(
            minutes: Int,
            vararg days: LocalDate,
        ) {
            widgets.forEach { widget ->
                // The summary counts one day; the on-screen table is only held for today, so any other day is not sent.
                val held = widget.kind != WidgetKind.DAY_SUMMARY || days.size == 1
                if (held && (widget.source != Source.ON_SCREEN || days.singleOrNull() == today)) {
                    numbers +=
                        DashboardInput.ForWidget(days.map(::bounds), widget.id) to
                        numbersOf(minutes, days.toList(), hidden)
                }
            }
        }
        add(todayMinutes, today)
        if (today != monday) add(30, today.minusDays(1))
        add(todayMinutes + if (today != monday) 30 else 0, *(0L..6L).map { monday.plusDays(it) }.toTypedArray())
        add(50, picked)
        if (today == monday) add(30, today.minusDays(1))
        store.applySync(
            CopyUpdate(
                computer = "test-computer",
                syncedAt = Instant.now().toString(),
                resetMarker = null,
                dashboard = DashboardUpdate("token", ZONE, document, numbers),
            ),
        )
    }

    private fun numbersOf(
        minutes: Int,
        days: List<LocalDate>,
        hidden: Int,
    ) = DashboardNumbers(
        totalMinutes = minutes,
        sessionCount = 3,
        days = days.map { DayMinutes(it.toString(), if (it == days.last()) minutes else 0) },
        groups = listOf(GroupMinutes("app", minutes - 35, 0.6, 2), GroupMinutes("site", 35, 0.4, 1)),
        hiddenProjectCount = hidden,
    )

    /** A day the way the computer counts it: its label, and its start and end in UTC minutes. */
    private fun bounds(date: LocalDate): DayBounds {
        val utc = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:00'Z'")

        fun at(day: LocalDate) = utc.format(day.atStartOfDay(zone).withZoneSameInstant(ZoneOffset.UTC))
        return DayBounds(date.toString(), at(date), at(date.plusDays(1)))
    }
}

private fun SemanticsNodeInteraction.assertTextIs(text: String) = assertTextEquals(text)

private fun SemanticsNodeInteraction.assertTextStartsWith(prefix: String) = assert(hasText(prefix, substring = true))
