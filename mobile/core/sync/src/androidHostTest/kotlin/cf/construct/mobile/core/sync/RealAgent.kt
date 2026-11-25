package cf.construct.mobile.core.sync

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.net.ServerSocket
import java.util.concurrent.TimeUnit

/**
 * The agent's own test program (`agent/examples/phone_pairing.rs`), started as a process on the loopback address, with
 * the made-up data of [data] read again at every command. [dir] keeps its certificate and its paired phones, so a
 * program started again on the same [dir] is the same computer, at the same port or at another.
 */
class RealAgent(
    binary: String,
    val dir: File,
    val data: File,
    port: Int = freePort(),
) {
    private val process: Process =
        ProcessBuilder(
            binary,
            "--dir",
            dir.path,
            "--bind",
            "127.0.0.1:$port",
            "--host",
            "127.0.0.1",
            "--data",
            data.path,
            "--control",
            "--minutes",
            "5",
        ).redirectErrorStream(true).start()
    private val output = BufferedReader(InputStreamReader(process.inputStream))

    /** The text the QR code of the pairing holds. */
    val payload: String

    init {
        var found: String? = null
        while (true) {
            val line = output.readLine() ?: error("The test program stopped before it was ready.")
            if (line.startsWith("payload: ")) found = line.removePrefix("payload: ")
            if (line == "ready") break
        }
        payload = checkNotNull(found)
    }

    /** `erase` and `unpair` as the program takes them on its input; it answers with one line. */
    fun control(command: String): String {
        process.outputStream.write("$command\n".toByteArray())
        process.outputStream.flush()
        return output.readLine().orEmpty()
    }

    /** Stops the program, as a computer that goes away does. */
    fun stop() {
        if (process.isAlive) {
            runCatching {
                process.outputStream.write("stop\n".toByteArray())
                process.outputStream.flush()
            }
            if (!process.waitFor(STOP_SECONDS, TimeUnit.SECONDS)) process.destroyForcibly()
        }
    }

    companion object {
        private const val STOP_SECONDS = 10L

        fun freePort(): Int = ServerSocket(0).use { it.localPort }
    }
}

/** The data file of the test program: the shape is in the header of the example. */
class MadeUpData(
    val file: File,
) {
    var facts = listOf<JsonObject>()
    var chats = listOf<JsonObject>()
    var sessions = listOf<JsonObject>()

    fun write(): MadeUpData {
        file.writeText(
            buildJsonObject {
                put("zone", "UTC")
                put(
                    "facts",
                    buildJsonObject {
                        put("version", facts.size + chats.size)
                        put("items", JsonArray(facts))
                    },
                )
                put("chats", JsonArray(chats))
                put("document", JsonNull)
                put("sessions", JsonArray(sessions))
            }.toString(),
        )
        return this
    }

    companion object {
        fun fact(
            id: String,
            value: JsonElement = JsonPrimitive("value of $id"),
        ) = buildJsonObject {
            put("fact_id", id)
            put("entity_kind", "skill")
            put("property", "skills")
            put("value", value)
            put("stored_at", "2026-11-01T10:00:00Z")
        }

        fun chat(
            id: String,
            title: String,
            vararg items: JsonObject,
        ) = chatOf(id, title, items.toList())

        fun chatOf(
            id: String,
            title: String,
            items: List<JsonObject>,
        ) = buildJsonObject {
            put("id", id)
            put("title", title)
            put("started", "2026-11-01T10:00:00Z")
            put("items", JsonArray(items))
        }

        fun session(
            project: String,
            vararg minutes: String,
        ) = buildJsonObject {
            put("tool", "editor")
            put("session", "s-$project")
            put("project", "/work/$project")
            put("ticket", "")
            put("minutes", JsonArray(minutes.map { JsonPrimitive(it) }))
        }
    }
}
