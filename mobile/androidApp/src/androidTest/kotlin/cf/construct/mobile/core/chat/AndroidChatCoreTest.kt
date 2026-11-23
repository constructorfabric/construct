package cf.construct.mobile.core.chat

import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.Collections
import kotlin.concurrent.thread

private class JvmFakeProvider(
    private val replies: List<Reply>,
) : FakeProvider {
    private val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    private val received = Collections.synchronizedList(mutableListOf<Request>())
    private val held = Collections.synchronizedList(mutableListOf<Socket>())

    override val baseUrl: String = "http://127.0.0.1:${server.localPort}"
    override val requests: List<Request> get() = synchronized(received) { received.toList() }

    init {
        thread(isDaemon = true) {
            for (reply in replies) {
                val socket = runCatching { server.accept() }.getOrNull() ?: return@thread
                val input = socket.getInputStream()
                received += readRequest { input.read() }
                when (reply) {
                    is Reply.Status -> {
                        socket.getOutputStream().apply {
                            write(responseBytes(reply))
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
