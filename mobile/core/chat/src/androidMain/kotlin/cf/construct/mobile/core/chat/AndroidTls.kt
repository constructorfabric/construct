package cf.construct.mobile.core.chat

import android.content.Context

/** The JNI entry of the Rust library: it hands the app's context to the system's certificate check. */
internal object AndroidTls {
    init {
        System.loadLibrary("construct_chat_bridge")
    }

    external fun nativeInit(context: Context)
}

/**
 * Prepares the chat core for `https` addresses: Android's certificate check is reached through the
 * JVM, so the app calls this once, in `Application.onCreate`, before the first request.
 */
fun initChatCore(context: Context) = AndroidTls.nativeInit(context.applicationContext)
