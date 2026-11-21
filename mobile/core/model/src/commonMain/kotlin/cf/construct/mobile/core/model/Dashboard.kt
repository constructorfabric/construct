package cf.construct.mobile.core.model

/** Whose time the page counts: the user's own, the unattended agents', or both. */
enum class Whose { OWN, UNATTENDED, ALL }

/** One day as the page counts it: its label and its start and end in UTC, whole minutes. */
data class DayBounds(
    val label: String,
    val start: String,
    val end: String,
)

/** What a widget keeps: a list left out means no limit on it. */
data class Filter(
    val projects: List<String>? = null,
    val tickets: List<String>? = null,
    val tools: List<String>? = null,
)

/** What to count in the days: a widget of the document, or the sessions and filter the call names. */
sealed interface DashboardInput {
    val days: List<DayBounds>

    data class ForWidget(
        override val days: List<DayBounds>,
        val widget: String,
    ) : DashboardInput

    data class ForSessions(
        override val days: List<DayBounds>,
        val sessions: Whose,
        val filter: Filter? = null,
    ) : DashboardInput
}

data class DayMinutes(
    val label: String,
    val minutes: Int,
)

/** One group of a table: its key, its time, its share of the total (0 to 1) and its sessions. */
data class GroupMinutes(
    val key: String,
    val minutes: Int,
    val share: Double,
    val sessions: Int,
)

/** The numbers of a period. They hold no text of a message. */
data class DashboardNumbers(
    val totalMinutes: Int,
    val sessionCount: Int,
    val days: List<DayMinutes>,
    val groups: List<GroupMinutes>,
)

/** What the agent answers while it builds the activity time from the history. */
const val READING_HISTORY_ERROR = "the activity time is being built"

/** Whether the numbers will come and the page should ask again. */
val Outcome.Failed.isReadingHistory: Boolean get() = error == READING_HISTORY_ERROR

enum class WidgetKind { TOTAL, BARS_PER_DAY, TABLE, DAY_SUMMARY }

enum class GroupBy { PROJECT, TICKET, TOOL, DAY }

enum class Period { TODAY, THIS_WEEK, LAST_WEEK, THIS_MONTH, LAST_MONTH, LAST_7_DAYS, LAST_30_DAYS }

/** Where a widget sits on the grid of 12 columns; columns and rows count from 0. */
data class Position(
    val column: Int,
    val row: Int,
    val width: Int,
    val height: Int,
)

data class Widget(
    val id: String,
    val kind: WidgetKind,
    val position: Position,
    val groupBy: GroupBy? = null,
    val period: Period? = null,
    val sessions: Whose? = null,
    val filter: Filter? = null,
)

data class DashboardDocument(
    val title: String,
    /** The period and the sessions of every widget that has none of its own. */
    val period: Period,
    val sessions: Whose,
    val widgets: List<Widget>,
)

/** Why the file as it is now is not used, and where: a line and column or a JSON pointer. */
data class DocumentProblem(
    val kind: Kind,
    val place: String,
    val message: String,
) {
    enum class Kind { FILE, LINE, POINTER }
}

/** The last good document, if there was one, and the problem of the file as it is now. */
data class DocumentState(
    val document: DashboardDocument?,
    val problem: DocumentProblem?,
)
