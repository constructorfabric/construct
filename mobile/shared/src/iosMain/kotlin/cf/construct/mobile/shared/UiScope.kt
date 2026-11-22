package cf.construct.mobile.shared

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel

/**
 * A coroutine scope on the main thread that Swift can create and close, for the state holders of the
 * features: a holder runs its reads in the scope it is given, and the screen's owner closes it.
 */
class UiScope : CoroutineScope by MainScope() {
    /** Cancels everything still running in the scope. */
    fun close() = cancel()
}
