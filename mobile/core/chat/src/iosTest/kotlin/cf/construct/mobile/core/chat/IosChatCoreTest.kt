@file:OptIn(ExperimentalForeignApi::class)

package cf.construct.mobile.core.chat

import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.driver.native.inMemoryDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.alloc
import kotlinx.cinterop.convert
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.reinterpret
import kotlinx.cinterop.sizeOf
import kotlinx.cinterop.usePinned
import kotlinx.cinterop.value
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import platform.posix.AF_INET
import platform.posix.SOCK_STREAM
import platform.posix.accept
import platform.posix.bind
import platform.posix.close
import platform.posix.getsockname
import platform.posix.listen
import platform.posix.recv
import platform.posix.send
import platform.posix.sockaddr
import platform.posix.sockaddr_in
import platform.posix.socket
import platform.posix.socklen_tVar
import kotlin.concurrent.AtomicReference
import kotlin.test.Test
import kotlin.test.assertEquals

/** A listening socket on a free local port: its descriptor and its port. */
private fun listenOnFreePort(): Pair<Int, Int> =
    memScoped {
        val fd = socket(AF_INET, SOCK_STREAM, 0)
        val address = alloc<sockaddr_in>()
        address.sin_family = AF_INET.convert()
        address.sin_port = 0.convert()
        // 127.0.0.1 in network byte order, read as a little-endian number (every Apple device is one).
        address.sin_addr.s_addr = 0x0100007Fu
        check(bind(fd, address.ptr.reinterpret<sockaddr>(), sizeOf<sockaddr_in>().convert()) == 0) { "bind failed" }
        val length = alloc<socklen_tVar>()
        length.value = sizeOf<sockaddr_in>().convert()
        getsockname(fd, address.ptr.reinterpret<sockaddr>(), length.ptr)
        check(listen(fd, 8) == 0) { "listen failed" }
        val port = address.sin_port.toInt()
        fd to (((port and 0xFF) shl 8) or (port shr 8))
    }

private fun readByte(fd: Int): Int {
    val buffer = ByteArray(1)
    val count = buffer.usePinned { recv(fd, it.addressOf(0), 1.convert(), 0) }
    return if (count <= 0) -1 else buffer[0].toInt()
}

private fun writeAll(
    fd: Int,
    bytes: ByteArray,
) {
    bytes.usePinned { send(fd, it.addressOf(0), bytes.size.convert(), 0) }
}

private class NativeFakeProvider(
    replies: List<Reply>,
) : FakeProvider {
    private val listener: Int
    private val received = AtomicReference<List<Request>>(emptyList())
    private val held = AtomicReference<List<Int>>(emptyList())
    override val baseUrl: String

    init {
        val (fd, port) = listenOnFreePort()
        listener = fd
        baseUrl = "http://127.0.0.1:$port"
        CoroutineScope(Dispatchers.Default).launch {
            for (reply in replies) {
                val client = accept(fd, null, null)
                if (client < 0) return@launch
                received.value = received.value + readRequest { readByte(client) }
                when (reply) {
                    is Reply.Status -> {
                        writeAll(client, responseBytes(reply))
                        close(client)
                    }

                    is Reply.Stream -> {
                        if (reply.holdOpen) held.value = held.value + client
                        writeAll(client, streamBytes(reply))
                        if (!reply.holdOpen) close(client)
                    }

                    Reply.Hang -> {
                        held.value = held.value + client
                    }
                }
            }
        }
    }

    override val requests: List<Request> get() = received.value

    override fun close() {
        close(listener)
        held.value.forEach { close(it) }
    }
}

private object NativeFakes : Fakes {
    override fun start(replies: List<Reply>): FakeProvider = NativeFakeProvider(replies)

    override fun closedAddress(): String {
        val (fd, port) = listenOnFreePort()
        close(fd)
        return "http://127.0.0.1:$port"
    }
}

class IosChatCoreTest : ChatCoreTestCases() {
    override val fakes: Fakes = NativeFakes

    @Test
    fun theNativeAnswerCallsBackWithRecordsAndProviderErrors() {
        val provider = fakes.start(listOf(Reply.Status(401, "{}")))
        val answer = newChatAnswer()
        try {
            val request =
                """
                {
                    "model": {"base_url":"${provider.baseUrl}", "model":"local-model", "api_key":"fake-key"},
                    "profile": {"categories":[]},
                    "history": [],
                    "request": {"conversation":"", "message":"Hello"}
                }
                """.trimIndent()
            val records = mutableListOf<String>()
            val events = mutableListOf<String>()
            answer.send(request, records::add, events::add)
            val record = Json.parseToJsonElement(records.single()).jsonObject
            assertEquals("Hello", record.getValue("text").jsonPrimitive.content)
            val reason = Json.parseToJsonElement(events.last()).jsonObject
            assertEquals("key refused", reason.getValue("reason").jsonPrimitive.content)
            assertEquals("Bearer fake-key", provider.requests.single().authorization)
        } finally {
            answer.close()
            provider.close()
        }
    }
}

class IosPhoneConstructTest : PhoneConstructTestCases() {
    override val fakes: Fakes = NativeFakes

    override fun openCopy(): SqlDriver = inMemoryDriver(ConstructDatabase.Schema)
}
