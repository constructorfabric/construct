@file:OptIn(ExperimentalForeignApi::class)

package cf.construct.mobile.core.chat

import cf.construct.mobile.core.chat.bridge.cf_chat_fixed_request_body
import cf.construct.mobile.core.chat.bridge.cf_chat_free
import cf.construct.mobile.core.chat.bridge.cf_chat_test_model
import kotlinx.cinterop.ByteVar
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.alloc
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.toKString
import kotlinx.cinterop.value

/** Reads and frees a string the library returned. */
private fun take(text: CPointer<ByteVar>?): String {
    val value = text?.toKString().orEmpty()
    cf_chat_free(text)
    return value
}

internal actual fun nativeFixedRequestBody(toolsFile: String): String =
    memScoped {
        val out = alloc<kotlinx.cinterop.CPointerVar<ByteVar>>()
        val code = cf_chat_fixed_request_body(toolsFile, out.ptr)
        val text = take(out.value)
        if (code != 0) throw ChatCoreException(text)
        text
    }

internal actual fun nativeTestModel(
    baseUrl: String,
    model: String,
    apiKey: String,
): String = take(cf_chat_test_model(baseUrl, model, apiKey))
