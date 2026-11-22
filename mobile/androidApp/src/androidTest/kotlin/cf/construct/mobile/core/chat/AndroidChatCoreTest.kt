package cf.construct.mobile.core.chat

import java.io.InputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.Collections
import kotlin.concurrent.thread

private class JvmFakeProvider(
    private val replies: List<Reply>,
) : FakeProvider {
    private val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    private val received = Collections.synchronizedList(mutableListOf<String>())
    private val held = Collections.synchronizedList(mutableListOf<Socket>())

    override val baseUrl: String = "http://127.0.0.1:${server.localPort}"
    override val bodies: List<String> get() = synchronized(received) { received.toList() }

    init {
        thread(isDaemon = true) {
            for (reply in replies) {
                val socket = runCatching { server.accept() }.getOrNull() ?: return@thread
                received += readBody(socket.getInputStream())
                when (reply) {
                    is Reply.Status -> {
                        val body = reply.body.toByteArray()
                        val head =
                            "HTTP/1.1 ${reply.code} X\r\nContent-Type: application/json\r\n" +
                                "Content-Length: ${body.size}\r\nConnection: close\r\n\r\n"
                        socket.getOutputStream().apply {
                            write(head.toByteArray() + body)
                            flush()
                        }
                        socket.close()
                    }

                    Reply.Hang -> {
                        held += socket
                    }
                }
            }
        }
    }

    override fun close() {
        server.close()
        synchronized(held) { held.forEach { runCatching { it.close() } } }
    }
}

/** The body of one request: the head up to the blank line, then `Content-Length` bytes. */
private fun readBody(input: InputStream): String {
    val head = StringBuilder()
    while (!head.endsWith("\r\n\r\n")) {
        val byte = input.read()
        if (byte < 0) return ""
        head.append(byte.toChar())
    }
    val length =
        Regex("(?i)content-length: *(\\d+)")
            .find(head)
            ?.groupValues
            ?.get(1)
            ?.toInt() ?: 0
    return String(input.readNBytes(length))
}

private object JvmFakes : Fakes {
    override fun start(replies: List<Reply>): FakeProvider = JvmFakeProvider(replies)

    override fun closedAddress(): String {
        val port = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }
        return "http://127.0.0.1:$port"
    }
}

class AndroidChatCoreTest : ChatCoreTestCases() {
    override val fakes: Fakes = JvmFakes
}
