package cf.construct.mobile.core.sync

import cf.construct.mobile.core.connection.ComputerChannel
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.put

/** The whole sync cannot go on now: the computer is erasing, or answered what a phone cannot use. */
internal class Cut : Exception("The sync was cut.")

/** One part cannot be had now (an error the computer gave for it); the sync goes on without it. */
internal class PartUnavailable : Exception("The computer cannot give this part now.")

/** The part changed while its pages were read: the pages do not fit together, and the part is read again. */
internal class PartChanged : Exception("The part changed while it was read.")

private const val ERASING = "erasing"

/** What the computer answered to a command that lists: a page of items, and where the next one starts. */
internal class Page(
    val token: String?,
    val items: List<JsonObject>,
    val next: Int?,
    val changed: Boolean,
)

/** The pages of one part put together, with the token the computer gave for them. */
internal class Pages(
    val token: String,
    val items: List<JsonObject>,
)

/** The answer of `sync-check`: a token for each part, and the marker of the computer's last erasure. */
internal class Check(
    val facts: String?,
    val chats: String?,
    val dashboard: String?,
    val reset: String?,
)

private fun JsonObject.failure() {
    if (get("ok") == JsonPrimitive(true)) return
    // The computer's error texts are fixed words, but what is answered is never put into this phone's own text.
    throw if ((get("error") as? JsonPrimitive)?.content == ERASING) Cut() else PartUnavailable()
}

internal fun ComputerChannel.check(): Check {
    val reply = ask(buildJsonObject { put("cmd", "sync-check") })
    // A check that fails is not a part that fails: nothing can be said of the parts, so the sync is cut.
    if (reply["ok"] != JsonPrimitive(true)) throw Cut()
    val tokens = reply["tokens"] as? JsonObject ?: throw Cut()
    return Check(
        facts = tokens.text("facts"),
        chats = tokens.text("chats"),
        dashboard = tokens.text("dashboard"),
        reset = reply.text("reset"),
    )
}

internal fun ComputerChannel.page(
    command: String,
    after: Int,
    token: String?,
    id: String?,
): Page {
    val reply =
        ask(
            buildJsonObject {
                put("cmd", command)
                if (after > 0) put("after", after)
                if (token != null) put("token", token)
                if (id != null) put("id", id)
            },
        )
    reply.failure()
    val items = (reply["items"] as? JsonArray).orEmpty().filterIsInstance<JsonObject>()
    val next = (reply["next"] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.intOrNull
    return Page(reply.text("token"), items, next, reply["changed"] == JsonPrimitive(true))
}

/**
 * Reads every page of a part, from position [from]. The token of the first page is held to on every later one,
 * so a part that changed meanwhile is [PartChanged] and not a mix of two states. A position that does not move
 * forward would loop for ever, and is a [Cut].
 */
internal fun ComputerChannel.pages(
    command: String,
    id: String? = null,
    from: Int = 0,
    token: String? = null,
    onItems: (Int) -> Unit = {},
): Pages {
    var after = from
    var pinned = token
    val items = mutableListOf<JsonObject>()
    while (true) {
        val page = page(command, after, pinned, id)
        pinned = page.tokenHeldTo(pinned)
        items += page.items
        onItems(items.size)
        val next = page.next ?: return Pages(pinned, items)
        if (next <= after) throw Cut()
        after = next
    }
}

/** The token of this page, once it is known to belong to the same state as the pages before it. */
private fun Page.tokenHeldTo(pinned: String?): String {
    val differs = pinned != null && token != null && token != pinned
    if (changed || differs) throw PartChanged()
    return token ?: throw Cut()
}

/** The dashboard's document and zone: one answer of its own, which is not counted in a page of items. */
internal class DocumentAnswer(
    val token: String,
    val document: JsonObject?,
    val zone: String,
)

internal fun ComputerChannel.document(): DocumentAnswer {
    val reply = ask(buildJsonObject { put("cmd", "sync-document") })
    reply.failure()
    return DocumentAnswer(
        token = reply.text("token") ?: throw Cut(),
        document = reply["document"] as? JsonObject,
        zone = reply.text("zone") ?: throw Cut(),
    )
}
