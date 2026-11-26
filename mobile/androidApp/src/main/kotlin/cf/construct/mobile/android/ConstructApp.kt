package cf.construct.mobile.android

import android.app.Application
import android.util.Log
import cf.construct.mobile.core.chat.initChatCore
import cf.construct.mobile.core.sync.Syncer

/** Holds the app's one [cf.construct.mobile.core.construct.Construct], so a new screen reuses its open copy. */
class ConstructApp : Application() {
    val modelSettings by lazy { createModelSettingsStore(this) }
    internal val copy by lazy { createCopy(this) }
    val construct by lazy { createConstruct(copy, modelSettings) }
    val chatCore by lazy { createChatCore() }
    val versionName: String by lazy { packageManager.getPackageInfo(packageName, 0).versionName.orEmpty() }
    val phoneConnection by lazy { createPhoneConnection(this, PhonePolling.intervalMillis, copy) }
    internal val syncer: Syncer by lazy { createSyncer(phoneConnection, copy) }

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
