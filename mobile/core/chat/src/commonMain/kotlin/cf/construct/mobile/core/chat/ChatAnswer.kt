package cf.construct.mobile.core.chat

/**
 * One native answer. [send] blocks its caller and invokes the callbacks on that same thread.
 * [stop] may run on a separate thread. [close] must wait for both calls to return.
 */
interface ChatAnswer {
    fun send(
        input: String,
        writeRecord: (String) -> Boolean,
        emitEvent: (String) -> Boolean,
    )

    fun stop()

    fun close()
}

expect fun newChatAnswer(): ChatAnswer

internal const val CHAT_EVENT = 0
internal const val CHAT_RECORD = 1
