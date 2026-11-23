package cf.construct.mobile.core.chat

import android.content.Context

/** The JNI entry of the Rust library: it hands the app's context to the system's certificate check. */
internal object AndroidTls {
    @Volatile
    var ready = false

    init {
        System.loadLibrary("construct_chat_bridge")
    }

    external fun nativeInit(context: Context)
}

/**
 * Prepares the chat core for `https` addresses: Android's certificate check is reached through the
 * JVM, so the app calls this once, in `Application.onCreate`, before the first request. It throws
 * when the library cannot be loaded or the init fails; the caller decides what that costs.
 */
fun initChatCore(context: Context) {
    AndroidTls.nativeInit(context.applicationContext)
    AndroidTls.ready = true
}

/** Whether [initChatCore] has run: an `https` address is refused as unreachable until it has. */
fun chatCoreIsReady(): Boolean = AndroidTls.ready
