package cf.construct.mobile.core.chat

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * The texts of the desktop app's Settings page (`OutcomeMessage.*` in its `en.json`), word for word.
 * A result the list does not know is shown as the core sent it.
 */
val desktopReasons: Map<String, String> =
    mapOf(
        "key refused" to "key refused: the provider did not accept the key.",
        "model not found" to "model not found: the provider has no model with this name.",
        "unreachable" to "unreachable: nothing answered at this address.",
        "timeout" to "timeout: the provider did not answer in time.",
        "tools refused" to "tools refused: the provider did not accept a request that offers tools.",
        "no tool call" to "no tool call: the model answered in text where it had to call the tool.",
        "bad tool call" to "bad tool call: the model called a tool in a way that does not fit.",
        "wrong answer" to "wrong answer: the model did not answer from the result of the tool.",
    )

private const val UNREADABLE = "Not done: the core's answer cannot be read."

/** Reads the last JSON line of a Test into [ModelTestResult]. */
internal fun interpretModelTest(line: String): ModelTestResult {
    val answer = runCatching { Json.parseToJsonElement(line).jsonObject }.getOrNull()
    val error = answer?.get("error")?.jsonPrimitive?.contentOrNull
    val result = answer?.get("result")?.jsonPrimitive?.contentOrNull
    return when {
        answer == null -> {
            ModelTestResult.Failed(UNREADABLE)
        }

        error != null -> {
            ModelTestResult.Failed("Not done: $error")
        }

        result == null -> {
            ModelTestResult.Failed(UNREADABLE)
        }

        result == "works" -> {
            ModelTestResult.Works(answer["text"]?.jsonPrimitive?.contentOrNull.orEmpty())
        }

        result == "error" -> {
            ModelTestResult.Failed(
                "error: the provider answered with status ${answer["status"]?.jsonPrimitive?.intOrNull}.",
            )
        }

        else -> {
            ModelTestResult.Failed(desktopReasons[result] ?: result)
        }
    }
}
