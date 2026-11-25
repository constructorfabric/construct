package cf.construct.mobile.core.sync

import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.EntityKind
import cf.construct.mobile.core.model.MemoryFact
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** What the copy shows for an item of a chat that this app cannot show: one line, and the chat goes on. */
const val UNKNOWN_ITEM_LINE = "Something here cannot be shown in this version of the app."

/** A fact of the computer, or null for one in a shape this app does not know: a kind of fact it has no name for. */
internal fun fact(item: JsonObject): MemoryFact? =
    readOrNull {
        item.text("kind").takeIf { it == "fact" }.required()
        MemoryFact(
            factId = item.text("id").required(),
            entityKind = enumOf<EntityKind>(item.text("entity_kind")).required(),
            property = item.text("property").required(),
            value = item["value"]?.takeUnless { it is JsonNull }.required(),
            storedAt = item.text("time").required(),
            tool = item.text("tool"),
        )
    }

/** A chat of the computer as the list gives it: how many lines it has decides what is still to be read. */
internal class ChatHeader(
    val id: String,
    val title: String,
    val started: String,
    val count: Int,
)

internal fun chatHeader(item: JsonObject): ChatHeader? =
    readOrNull {
        item.text("kind").takeIf { it == "chat" }.required()
        ChatHeader(
            id = item.text("id").required(),
            title = item.text("title").required(),
            started = item.text("started").required(),
            count = item.number("count")?.takeIf { it >= 0 }.required(),
        )
    }

/** One line of a chat: its time, when it has one, and what the phone shows for it. */
internal class ChatLine(
    val time: String?,
    val message: ChatMessage,
)

/**
 * The line for one item of a chat. It is always one line, so the lines the copy holds are as many as the items
 * the computer counts: an item of a kind this app does not know, or one that lacks what it needs, is
 * [UNKNOWN_ITEM_LINE] and never stops a sync.
 */
internal fun chatLine(item: JsonObject): ChatLine {
    val time = item.text("time")
    val known =
        when (item.text("kind")) {
            "message" -> {
                val role =
                    when (item.text("role")) {
                        "user" -> ChatRole.USER
                        "assistant" -> ChatRole.ASSISTANT
                        else -> null
                    }
                val text = item.text("text")
                if (role != null && text != null) ChatMessage(role, text) else null
            }

            "tool" -> {
                item.text("line")?.let { ChatMessage(ChatRole.TOOL, it) }
            }

            else -> {
                null
            }
        }
    return ChatLine(time, known ?: ChatMessage(ChatRole.TOOL, UNKNOWN_ITEM_LINE))
}
