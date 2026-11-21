package cf.construct.mobile.android

import android.app.Application

/** Holds the app's one [cf.construct.mobile.core.construct.Construct], so a new screen reuses its open copy. */
class ConstructApp : Application() {
    val construct by lazy { createConstruct(this) }
}
