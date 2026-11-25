package cf.construct.mobile.core.sync

import cf.construct.mobile.core.connection.ComputerAccess
import cf.construct.mobile.core.connection.ComputerChannel
import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.ComputerLost
import cf.construct.mobile.core.connection.ComputerReach
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * A computer that answers the sync commands as the agent does (the format is in `agent/src/phone/sync.rs`): a
 * token for each part that changes with the part, pages of [pageSize] items, a `changed` answer for a page asked
 * with the token of an older state, a reset marker, and the error `erasing`.
 */
class FakeComputer(
    var identity: String = "computer-a",
) : ComputerAccess {
    var facts = listOf<JsonObject>()
    var chats = linkedMapOf<String, FakeChat>()
    var numbers = listOf<JsonObject>()
    var document: JsonObject? = null
    var zone = "Europe/Berlin"
    var reset: String? = "marker-1"
    var erasing = false
    var pageSize = 2

    /** Set to make the computer unreachable for the reason; nothing is run then. */
    var missed: ComputerLink? = null

    /** The commands that answer an error of their own, as the agent does for a part it cannot read now. */
    var failing = setOf<String>()

    /** The words the computer puts in the error of a command in [failing]. */
    var errorText = "unavailable"

    /** Called with the command before it is answered; a test changes the data here to change it mid-read. */
    var beforeAnswer: (String) -> Unit = {}

    /** The channel is lost when this many requests were asked in all; null never. */
    var lostAfter: Int? = null

    val requests = mutableListOf<JsonObject>()

    class FakeChat(
        var title: String,
        val started: String,
        var items: List<JsonObject>,
    )

    fun commands(): List<String> = requests.map { it["cmd"]!!.jsonPrimitive.content }

    fun dataCommands(): List<String> = commands().filter { it != "sync-check" }

    override suspend fun <T> reach(block: (ComputerChannel) -> T): ComputerReach<T> {
        missed?.let { return ComputerReach.Missed(it) }
        return try {
            ComputerReach.Reached(block(Channel()))
        } catch (_: ComputerLost) {
            ComputerReach.Missed(ComputerLink.Unreachable)
        }
    }

    private fun factsToken() = "facts-${facts.hashCode()}"

    private fun chatsToken() = "chats-${chats.map { it.key to (it.value.title to it.value.items) }.hashCode()}"

    private fun dashboardToken() = "dashboard-${listOf(numbers, document, zone).hashCode()}"

    private inner class Channel : ComputerChannel {
        override val identity: String get() = this@FakeComputer.identity

        override fun ask(request: JsonObject): JsonObject {
            if (lostAfter?.let { requests.size >= it } == true) throw ComputerLost()
            requests += request
            val command = request["cmd"]!!.jsonPrimitive.content
            beforeAnswer(command)
            return when {
                erasing -> failure("erasing")
                command in failing -> failure("$command $errorText")
                else -> answer(command, request)
            }
        }
    }

    private fun answer(
        command: String,
        request: JsonObject,
    ): JsonObject =
        when (command) {
            "sync-check" -> {
                checkAnswer()
            }

            "sync-facts" -> {
                part(request, factsToken(), facts)
            }

            "sync-chats" -> {
                part(request, chatsToken(), chats.map { (id, chat) -> header(id, chat) })
            }

            "sync-chat" -> {
                part(request, chatsToken(), chats[request["id"]!!.jsonPrimitive.content]?.items.orEmpty())
            }

            "sync-document" -> {
                documentAnswer()
            }

            "sync-dashboard" -> {
                part(request, dashboardToken(), numbers)
            }

            else -> {
                failure("unknown command")
            }
        }

    private fun failure(text: String): JsonObject = buildJsonObject { put("error", text) }

    private fun checkAnswer(): JsonObject =
        buildJsonObject {
            put("ok", true)
            put("format", 1)
            put(
                "tokens",
                buildJsonObject {
                    put("facts", factsToken())
                    put("chats", chatsToken())
                    put("dashboard", dashboardToken())
                },
            )
            reset?.let { put("reset", it) }
        }

    private fun header(
        id: String,
        chat: FakeChat,
    ) = buildJsonObject {
        put("kind", "chat")
        put("id", id)
        put("title", chat.title)
        put("started", chat.started)
        put("count", chat.items.size)
    }

    private fun documentAnswer(): JsonObject =
        buildJsonObject {
            put("ok", true)
            put("format", 1)
            put("token", dashboardToken())
            put("document", document ?: JsonNull)
            put("zone", zone)
        }

    private fun part(
        request: JsonObject,
        token: String,
        items: List<JsonObject>,
    ): JsonObject {
        val asked = (request["token"] as? JsonPrimitive)?.content
        if (asked != null && asked != token) {
            return buildJsonObject {
                put("ok", true)
                put("changed", true)
                put("token", token)
                put("items", JsonArray(emptyList()))
                put("next", JsonNull)
            }
        }
        val after = (request["after"] as? JsonPrimitive)?.int ?: 0
        val page = items.drop(after).take(pageSize)
        val next = (after + page.size).takeIf { it < items.size }
        return buildJsonObject {
            put("ok", true)
            put("format", 1)
            put("token", token)
            put("items", JsonArray(page))
            put("next", next?.let { JsonPrimitive(it) } ?: JsonNull)
        }
    }
}

fun fact(
    id: String,
    value: JsonElement = JsonPrimitive("value of $id"),
    kind: String = "skill",
) = buildJsonObject {
    put("kind", "fact")
    put("id", id)
    put("entity_kind", kind)
    put("property", "skills")
    put("value", value)
    put("time", "2026-11-01T10:00:00Z")
}

fun message(
    role: String,
    text: String,
    time: String = "2026-11-01T10:00:00Z",
) = buildJsonObject {
    put("kind", "message")
    put("role", role)
    put("time", time)
    put("text", text)
}

fun toolLine(
    line: String,
    time: String = "2026-11-01T10:00:00Z",
) = buildJsonObject {
    put("kind", "tool")
    put("time", time)
    put("line", line)
}

fun numbersItem(
    widget: String,
    day: String,
    minutes: Int,
) = buildJsonObject {
    put("kind", "numbers")
    put("period", "day")
    put("widget", widget)
    put(
        "input",
        buildJsonObject {
            put("widget", widget)
            put(
                "days",
                JsonArray(
                    listOf(
                        buildJsonObject {
                            put("label", day)
                            put("start", "${day}T00:00:00Z")
                            put("end", "${day}T23:59:00Z")
                        },
                    ),
                ),
            )
        },
    )
    put(
        "answer",
        buildJsonObject {
            // The computer counts minutes as decimals.
            put("total_minutes", minutes.toDouble())
            put("session_count", 1)
            put("hidden_project_count", 0)
            put("groups", JsonArray(emptyList()))
            put(
                "days",
                JsonArray(
                    listOf(
                        buildJsonObject {
                            put("label", day)
                            put("minutes", minutes.toDouble())
                        },
                    ),
                ),
            )
        },
    )
}
