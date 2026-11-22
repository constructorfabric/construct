package cf.construct.mobile.core.chat

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.ptr.PointerByReference

/** The C interface of `libconstruct_chat_bridge.so` (`chat-bridge/include/construct_chat.h`). */
@Suppress("FunctionNaming", "ktlint:standard:function-naming")
private interface Bridge : Library {
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
}

private val bridge: Bridge by lazy { Native.load("construct_chat_bridge", Bridge::class.java) }

/** A NUL-terminated UTF-8 copy: JNA would otherwise encode a String with the platform's charset. */
private fun cString(text: String): ByteArray = text.encodeToByteArray() + 0

/** Reads and frees a string the library returned. */
private fun take(text: Pointer): String =
    try {
        text.getString(0, "UTF-8")
    } finally {
        bridge.cf_chat_free(text)
    }

internal actual fun nativeFixedRequestBody(toolsFile: String): String {
    val out = PointerByReference()
    val code = bridge.cf_chat_fixed_request_body(cString(toolsFile), out)
    val text = take(out.value)
    if (code != 0) throw ChatCoreException(text)
    return text
}

internal actual fun nativeTestModel(
    baseUrl: String,
    model: String,
    apiKey: String,
): String = take(bridge.cf_chat_test_model(cString(baseUrl), cString(model), cString(apiKey)))
