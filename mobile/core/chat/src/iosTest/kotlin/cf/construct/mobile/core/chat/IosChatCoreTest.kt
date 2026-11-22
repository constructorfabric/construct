@file:OptIn(ExperimentalForeignApi::class)

package cf.construct.mobile.core.chat

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

/** The body of one request: the head up to the blank line, then `Content-Length` bytes. */
private fun readBody(fd: Int): String {
    val head = StringBuilder()
    while (!head.endsWith("\r\n\r\n")) {
        val byte = readByte(fd)
        if (byte < 0) return ""
        head.append(byte.toChar())
    }
    val length =
        Regex("(?i)content-length: *(\\d+)")
            .find(head)
            ?.groupValues
            ?.get(1)
            ?.toInt() ?: 0
    val bytes = ByteArray(length) { readByte(fd).toByte() }
    return bytes.decodeToString()
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
    private val received = AtomicReference<List<String>>(emptyList())
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
                received.value = received.value + readBody(client)
                when (reply) {
                    is Reply.Status -> {
                        val body = reply.body.encodeToByteArray()
                        val head =
                            "HTTP/1.1 ${reply.code} X\r\nContent-Type: application/json\r\n" +
                                "Content-Length: ${body.size}\r\nConnection: close\r\n\r\n"
                        writeAll(client, head.encodeToByteArray() + body)
                        close(client)
                    }

                    Reply.Hang -> {
                        held.value = held.value + client
                    }
                }
            }
        }
    }

    override val bodies: List<String> get() = received.value

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
}
