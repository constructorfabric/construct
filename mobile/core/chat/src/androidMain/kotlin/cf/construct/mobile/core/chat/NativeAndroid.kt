package cf.construct.mobile.core.chat

import com.sun.jna.Callback
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.ptr.PointerByReference

/** The chat and phone entry points use one loaded `libconstruct_chat_bridge.so`. */
@Suppress("FunctionNaming", "ktlint:standard:function-naming", "TooManyFunctions")
internal interface Bridge : Library {
    fun cf_chat_fixed_request_body(
        toolsFile: ByteArray,
        out: PointerByReference,
    ): Int

    fun cf_chat_test_model(
        baseUrl: ByteArray,
        model: ByteArray,
        apiKey: ByteArray,
    ): Pointer

    fun cf_chat_free(text: Pointer)

    fun cf_chat_stop_new(): Pointer

    fun cf_chat_stop_answer(stop: Pointer)

    fun cf_chat_stop_free(stop: Pointer)

    fun cf_chat_send(
        input: ByteArray,
        stop: Pointer,
        callback: AnswerCallback,
        context: Pointer?,
    )

    fun cf_phone_is_numeric_address(host: ByteArray): Byte

    fun cf_phone_open(
        host: ByteArray,
        port: Short,
        pin: ByteArray?,
        out: PointerByReference,
    ): Int

    fun cf_phone_fingerprint(channel: Pointer): Pointer?

    fun cf_phone_exchange(
        channel: Pointer,
        line: ByteArray,
        out: PointerByReference,
    ): Int

    fun cf_phone_pair_code(
        channel: Pointer,
        code: ByteArray,
        version: Long,
        name: ByteArray,
        out: PointerByReference,
    ): Int

    fun cf_phone_close(channel: Pointer)

    fun cf_phone_proof(
        key: ByteArray,
        fingerprint: ByteArray,
        agent: Byte,
    ): Pointer?
}

internal fun interface AnswerCallback : Callback {
    fun write(
        context: Pointer?,
        kind: Int,
        text: Pointer,
    ): Int
}

internal val bridge: Bridge by lazy { Native.load("construct_chat_bridge", Bridge::class.java) }

/** A NUL-terminated UTF-8 copy: JNA would otherwise encode a String with the platform's charset. */
internal fun cString(text: String): ByteArray = text.encodeToByteArray() + 0

/** Reads and frees a string the library returned. */
internal fun take(text: Pointer?): String? =
    text?.let {
        try {
            it.getString(0, "UTF-8")
        } finally {
            bridge.cf_chat_free(it)
        }
    }

internal actual fun nativeFixedRequestBody(toolsFile: String): String {
    val out = PointerByReference()
    val code = bridge.cf_chat_fixed_request_body(cString(toolsFile), out)
    val text = take(out.value).orEmpty()
    if (code != 0) throw ChatCoreException(text)
    return text
}

internal actual fun nativeTestModel(
    baseUrl: String,
    model: String,
    apiKey: String,
): String = take(bridge.cf_chat_test_model(cString(baseUrl), cString(model), cString(apiKey))).orEmpty()

actual fun newChatAnswer(): ChatAnswer =
    object : ChatAnswer {
        private val token =
            try {
                bridge.cf_chat_stop_new()
            } catch (_: UnsatisfiedLinkError) {
                throw ChatCoreException("the chat core could not load")
            }

        override fun send(
            input: String,
            writeRecord: (String) -> Boolean,
            emitEvent: (String) -> Boolean,
        ) {
            val callback =
                AnswerCallback { _, kind, pointer ->
                    val text = pointer.getString(0, "UTF-8")
                    val ok =
                        when (kind) {
                            CHAT_EVENT -> emitEvent(text)
                            CHAT_RECORD -> writeRecord(text)
                            else -> false
                        }
                    if (ok) 1 else 0
                }
            bridge.cf_chat_send(cString(input), token, callback, null)
        }

        override fun stop() = bridge.cf_chat_stop_answer(token)

        override fun close() = bridge.cf_chat_stop_free(token)
    }
