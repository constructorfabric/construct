package cf.construct.mobile.core.chat

/** Reads one request from [readByte], which returns -1 at the end of the stream. Both platforms use it. */
fun readRequest(readByte: () -> Int): Request {
    val head = StringBuilder()
    while (!head.endsWith("\r\n\r\n")) {
        val byte = readByte()
        if (byte < 0) return Request(head.toString(), "")
        head.append(byte.toChar())
    }
    val length =
        Regex("(?i)content-length: *(\\d+)")
            .find(head)
            ?.groupValues
            ?.get(1)
            ?.toInt() ?: 0
    val bytes = ByteArray(length) { readByte().toByte() }
    return Request(head.toString(), bytes.decodeToString())
}

/** The bytes of the answer to a request: the status line, the headers and the body. */
fun responseBytes(reply: Reply.Status): ByteArray {
    val body = reply.body.encodeToByteArray()
    val head =
        "HTTP/1.1 ${reply.code} X\r\nContent-Type: application/json\r\n" +
            "Content-Length: ${body.size}\r\nConnection: close\r\n\r\n"
    return head.encodeToByteArray() + body
}

fun streamBytes(reply: Reply.Stream): ByteArray =
    ("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n" + reply.body)
        .encodeToByteArray()
