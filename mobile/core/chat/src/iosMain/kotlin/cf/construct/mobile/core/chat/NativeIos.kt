@file:OptIn(ExperimentalForeignApi::class)

package cf.construct.mobile.core.chat

import cf.construct.mobile.core.chat.bridge.cf_chat_fixed_request_body
import cf.construct.mobile.core.chat.bridge.cf_chat_free
import cf.construct.mobile.core.chat.bridge.cf_chat_send
import cf.construct.mobile.core.chat.bridge.cf_chat_stop_answer
import cf.construct.mobile.core.chat.bridge.cf_chat_stop_free
import cf.construct.mobile.core.chat.bridge.cf_chat_stop_new
import cf.construct.mobile.core.chat.bridge.cf_chat_test_model
import kotlinx.cinterop.ByteVar
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.StableRef
import kotlinx.cinterop.alloc
import kotlinx.cinterop.asStableRef
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.staticCFunction
import kotlinx.cinterop.toKString
import kotlinx.cinterop.value

/** Reads and frees a string the library returned. */
internal fun take(text: CPointer<ByteVar>?): String? =
    try {
        text?.toKString()
    } finally {
        cf_chat_free(text)
    }

internal actual fun nativeFixedRequestBody(toolsFile: String): String =
    memScoped {
        val out = alloc<kotlinx.cinterop.CPointerVar<ByteVar>>()
        val code = cf_chat_fixed_request_body(toolsFile, out.ptr)
        val text = take(out.value).orEmpty()
        if (code != 0) throw ChatCoreException(text)
        text
    }

internal actual fun nativeTestModel(
    baseUrl: String,
    model: String,
    apiKey: String,
): String = take(cf_chat_test_model(baseUrl, model, apiKey)).orEmpty()

private class AnswerCallbacks(
    val writeRecord: (String) -> Boolean,
    val emitEvent: (String) -> Boolean,
)

actual fun newChatAnswer(): ChatAnswer =
    object : ChatAnswer {
        private val token = checkNotNull(cf_chat_stop_new())

        override fun send(
            input: String,
            writeRecord: (String) -> Boolean,
            emitEvent: (String) -> Boolean,
        ) {
            val callbacks = StableRef.create(AnswerCallbacks(writeRecord, emitEvent))
            try {
                cf_chat_send(
                    input,
                    token,
                    staticCFunction { context, kind, pointer ->
                        val host = context?.asStableRef<AnswerCallbacks>()?.get()
                        val text = pointer?.toKString()
                        val ok =
                            if (host == null || text == null) {
                                false
                            } else {
                                when (kind) {
                                    CHAT_EVENT -> host.emitEvent(text)
                                    CHAT_RECORD -> host.writeRecord(text)
                                    else -> false
                                }
                            }
                        if (ok) 1 else 0
                    },
                    callbacks.asCPointer(),
                )
            } finally {
                callbacks.dispose()
            }
        }

        override fun stop() = cf_chat_stop_answer(token)

        override fun close() = cf_chat_stop_free(token)
    }
