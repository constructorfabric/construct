package cf.construct.mobile.android

import android.util.Log
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.seedWithSampleData

/**
 * Debug builds have no sync yet, so an empty copy gets the sample data. A copy that cannot be
 * written is logged and left as it is: the reads then fail like any other read of that copy.
 */
@Suppress("TooGenericExceptionCaught")
internal fun seedCopy(store: CopyStore) {
    try {
        store.seedWithSampleData()
    } catch (failure: Exception) {
        Log.w("SeedCopy", "The sample data could not be written", failure)
    }
}
