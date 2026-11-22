package cf.construct.mobile.shared

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers

/** The dispatcher the phone's copy reads on, which Swift cannot reach as `Dispatchers.Default`. */
fun backgroundDispatcher(): CoroutineDispatcher = Dispatchers.Default
