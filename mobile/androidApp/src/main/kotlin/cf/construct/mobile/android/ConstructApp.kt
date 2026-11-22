package cf.construct.mobile.android

import android.app.Application
import cf.construct.mobile.core.chat.initChatCore

/** Holds the app's one [cf.construct.mobile.core.construct.Construct], so a new screen reuses its open copy. */
class ConstructApp : Application() {
    val construct by lazy { createConstruct(this) }

    override fun onCreate() {
        super.onCreate()
        // The chat core checks the certificate of an https model address through Android.
        initChatCore(this)
    }
}
