package cf.construct.mobile.core.sync

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlin.math.roundToInt

// Readers of the fields of what the computer sent: a field that is not there, or not of the kind asked for, is null.

internal fun JsonObject.text(name: String): String? = (get(name) as? JsonPrimitive)?.takeIf { it.isString }?.content

internal fun JsonObject.number(name: String): Int? =
    (get(name) as? JsonPrimitive)
        ?.takeUnless {
            it.isString
        }?.intOrNull

internal fun JsonObject.decimal(name: String): Double? =
    (get(name) as? JsonPrimitive)?.takeUnless { it.isString }?.doubleOrNull

/** An item of the computer that lacks what the phone needs of it: its reader gives null and the sync goes on. */
internal class Unreadable : Exception("The item is not in a shape this app knows.")

/** The value, or the end of the reader that asked for it, see [readOrNull]. */
internal fun <T : Any> T?.required(): T = this ?: throw Unreadable()

internal inline fun <T : Any> readOrNull(read: () -> T): T? =
    try {
        read()
    } catch (_: Unreadable) {
        null
    }

/** A time in minutes. The computer counts it as a decimal (`3.0`); the copy keeps whole minutes. */
internal fun JsonObject.minutes(name: String): Int? = decimal(name)?.takeIf { it.isFinite() && it >= 0 }?.roundToInt()

internal inline fun <reified E : Enum<E>> enumOf(text: String?): E? =
    enumValues<E>().firstOrNull { it.name.equals(text, ignoreCase = true) }

internal fun JsonObject.items(name: String): List<JsonObject> =
    (get(name) as? JsonArray).orEmpty().filterIsInstance<JsonObject>()
