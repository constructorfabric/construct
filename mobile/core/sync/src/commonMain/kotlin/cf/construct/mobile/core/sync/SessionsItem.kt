package cf.construct.mobile.core.sync

import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.GroupSession
import kotlinx.serialization.json.JsonObject

/** The sessions behind one group of one entry (a long group comes in several items, in order). */
internal class SessionsItem(
    val input: DashboardInput,
    val group: String,
    val rows: List<GroupSession>,
)

internal fun sessionsItem(item: JsonObject): SessionsItem? =
    readOrNull {
        item.text("kind").takeIf { it == "sessions" }.required()
        val input = (item["input"] as? JsonObject).required()
        SessionsItem(
            DashboardInput.ForWidget(input.items("days").map(::dayBounds), input.text("widget").required()),
            item.text("group").required(),
            item.items("rows").map(::groupSession),
        )
    }

private fun groupSession(row: JsonObject) =
    GroupSession(
        start = row.text("start").required(),
        end = row.text("end").required(),
        minutes = row.number("minutes").required(),
        tool = row.text("tool").required(),
        project = row.text("project").required(),
        ticket = row.text("ticket").required(),
        session = row.text("session").required(),
    )

/** The numbers with the sessions the computer sent for them; numbers it sent none for keep none. */
internal fun withSessions(
    numbers: List<Pair<DashboardInput, DashboardNumbers>>,
    items: List<SessionsItem>,
): List<Pair<DashboardInput, DashboardNumbers>> {
    val byInput = items.groupBy { it.input }
    return numbers.map { (input, held) ->
        val sent = byInput[input] ?: return@map input to held
        input to held.copy(sessionsByGroup = sent.groupBy({ it.group }, { it.rows }).mapValues { it.value.flatten() })
    }
}
