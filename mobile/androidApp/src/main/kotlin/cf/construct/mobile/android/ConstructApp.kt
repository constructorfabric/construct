package cf.construct.mobile.android

import android.app.Application
import android.util.Log
import cf.construct.mobile.core.chat.initChatCore

/** Holds the app's one [cf.construct.mobile.core.construct.Construct], so a new screen reuses its open copy. */
class ConstructApp : Application() {
    val construct by lazy { createConstruct(this) }

    // The init throws a plain RuntimeException from Java when the check cannot start.
    @Suppress("TooGenericExceptionCaught")
    override fun onCreate() {
        super.onCreate()
        // The chat core checks the certificate of an https model address through Android. A failure
        // here must not take the app down (the memory screens do not need the core). If the init failed,
        // https addresses come back as unreachable; if the library did not load, a call to the core
        // throws. The log says why.
        try {
            initChatCore(this)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "The chat core library did not load", e)
        } catch (e: RuntimeException) {
            Log.e(TAG, "The chat core's certificate check did not start", e)
        }
    }
}

private const val TAG = "ConstructApp"
