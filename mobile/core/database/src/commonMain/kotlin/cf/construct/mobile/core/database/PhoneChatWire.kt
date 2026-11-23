package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.NO_CHAT_MODEL
import cf.construct.mobile.core.keys.ProviderSettings
import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.MemoryFacts
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

internal const val PHONE_ORIGIN = "phone"
internal const val COMPUTER_READ_ONLY = "this conversation was started on the computer and is read-only"

internal class PhoneRecord(
    val session: String,
    val time: String,
    val message: ChatMessage?,
) {
    companion object {
        fun read(line: String): PhoneRecord {
            val record = Json.parseToJsonElement(line).jsonObject
            val message =
                if (record.text("kind") == "turn") {
                    ChatMessage(ChatRole.valueOf(record.text("role").uppercase()), record.text("text"))
                } else {
                    null
                }
            return PhoneRecord(record.text("session"), record.text("time"), message)
        }
    }
}

internal fun answerRequest(
    input: ChatInput,
    settings: ProviderSettings,
    history: List<String>,
    facts: MemoryFacts,
): String =
    buildJsonObject {
        putJsonObject("model") {
            put("base_url", settings.address)
            put("model", settings.model)
            put("api_key", settings.key)
        }
        putJsonObject("request") {
            put("conversation", input.conversation)
            put("message", input.message)
        }
        putJsonArray("history") {
            history.forEach { add(Json.parseToJsonElement(it)) }
        }
        put("profile", profile(facts))
    }.toString()

private fun profile(facts: MemoryFacts): JsonObject =
    buildJsonObject {
        putJsonArray("categories") {
            facts.facts.groupBy { it.entityKind }.forEach { (kind, factsOfKind) ->
                add(
                    buildJsonObject {
                        put("entity_kind", kind.name.lowercase())
                        putJsonArray("facts") {
                            factsOfKind.forEach { fact ->
                                add(
                                    buildJsonObject {
                                        put("property", fact.property)
                                        put("value", fact.value)
                                        put("stored_at", fact.storedAt)
                                    },
                                )
                            }
                        }
                    },
                )
            }
        }
    }

internal fun chatEvent(line: String): ChatEvent {
    val event = Json.parseToJsonElement(line).jsonObject
    return when (event.text("event")) {
        "start" -> ChatEvent.Start(event.text("conversation"))
        "delta" -> ChatEvent.Delta(event.text("text"))
        "tool" -> ChatEvent.Tool(event.text("line"))
        "done" -> ChatEvent.Done(event["stopped"]?.jsonPrimitive?.booleanOrNull ?: false)
        "error" -> ChatEvent.Error(event.text("reason").let { if (it == "no model") NO_CHAT_MODEL else it })
        else -> error("unknown chat event")
    }
}

private fun JsonObject.text(key: String): String = getValue(key).jsonPrimitive.content
