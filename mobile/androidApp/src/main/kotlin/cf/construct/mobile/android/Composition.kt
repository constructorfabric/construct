package cf.construct.mobile.android

import android.content.Context
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.DatabaseConstruct
import cf.construct.mobile.core.database.androidCopyDriver

/**
 * The one place the app's objects are built. To run the app on [cf.construct.mobile.core.construct.FakeConstruct]
 * instead of the phone's copy, change [createConstruct] and nothing else.
 */
internal fun createConstruct(context: Context): Construct {
    val store = CopyStore(androidCopyDriver(context))
    seedCopy(store)
    return DatabaseConstruct(store)
}
