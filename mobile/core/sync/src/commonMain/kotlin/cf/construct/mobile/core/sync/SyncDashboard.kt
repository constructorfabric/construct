package cf.construct.mobile.core.sync

import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.DayBounds
import cf.construct.mobile.core.model.DayMinutes
import cf.construct.mobile.core.model.Filter
import cf.construct.mobile.core.model.GroupBy
import cf.construct.mobile.core.model.GroupMinutes
import cf.construct.mobile.core.model.Period
import cf.construct.mobile.core.model.Position
import cf.construct.mobile.core.model.Source
import cf.construct.mobile.core.model.Whose
import cf.construct.mobile.core.model.Widget
import cf.construct.mobile.core.model.WidgetKind
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * The document of the computer's dashboard in the types of the phone. A widget the phone cannot hold (a kind,
 * a grouping or a period it has no name for) is left out and the others stay; a document that lacks what the
 * phone needs of the whole is null.
 */
internal fun dashboardDocument(document: JsonObject): DashboardDocument? =
    readOrNull {
        val payload = (document["payload"] as? JsonObject).required()
        DashboardDocument(
            title = document.text("title").required(),
            period = enumOf<Period>(payload.text("period")).required(),
            sessions = enumOf<Whose>(payload.text("sessions")).required(),
            widgets = payload.items("widgets").mapNotNull(::widget),
            source = payload.text("source")?.let { enumOf<Source>(it).required() },
        )
    }

private fun widget(item: JsonObject): Widget? =
    readOrNull {
        val place = (item["position"] as? JsonObject).required()
        Widget(
            id = item.text("id").required(),
            kind = enumOf<WidgetKind>(item.text("kind")).required(),
            position =
                Position(
                    column = place.number("column").required(),
                    row = place.number("row").required(),
                    width = place.number("width").required(),
                    height = place.number("height").required(),
                ),
            groupBy = item.text("group_by")?.let { enumOf<GroupBy>(it).required() },
            period = item.text("period")?.let { enumOf<Period>(it).required() },
            sessions = item.text("sessions")?.let { enumOf<Whose>(it).required() },
            filter = (item["filter"] as? JsonObject)?.let(::filter),
            source = item.text("source")?.let { enumOf<Source>(it).required() },
        )
    }

private fun filter(item: JsonObject) =
    Filter(
        projects = strings(item["projects"]),
        tickets = strings(item["tickets"]),
        tools = strings(item["tools"]),
    )

private fun strings(element: Any?): List<String>? =
    (element as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { text -> text.isString }?.content }

/**
 * The numbers of one widget over one day or week, under the input they were counted for, or null for an entry that
 * has no answer (the computer could not count it) or one in a shape the phone does not know.
 */
internal fun dashboardNumbers(item: JsonObject): Pair<DashboardInput, DashboardNumbers>? =
    readOrNull {
        item.text("kind").takeIf { it == "numbers" }.required()
        val input = (item["input"] as? JsonObject).required()
        val answer = (item["answer"] as? JsonObject).required()
        DashboardInput.ForWidget(input.items("days").map(::dayBounds), input.text("widget").required()) to
            DashboardNumbers(
                totalMinutes = answer.minutes("total_minutes").required(),
                sessionCount = answer.number("session_count").required(),
                days = answer.items("days").map(::dayMinutes),
                groups = answer.items("groups").map(::groupMinutes),
                hiddenProjectCount = answer.number("hidden_project_count") ?: 0,
            )
    }

internal fun dayBounds(day: JsonObject) =
    DayBounds(day.text("label").required(), day.text("start").required(), day.text("end").required())

private fun dayMinutes(day: JsonObject) = DayMinutes(day.text("label").required(), day.minutes("minutes").required())

private fun groupMinutes(group: JsonObject) =
    GroupMinutes(
        key = group.text("key").required(),
        minutes = group.minutes("minutes").required(),
        share = group.decimal("share").required(),
        sessions = group.number("session_count").required(),
    )
