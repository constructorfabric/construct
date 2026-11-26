package cf.construct.mobile.feature.glasshouse

import android.app.Activity
import android.content.ContextWrapper
import android.view.WindowManager.LayoutParams.FLAG_SECURE
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext

/**
 * Marks the app's window secure while this is in the composition: the phone then leaves it out of screenshots, screen
 * recordings, other displays and the picture of the app in the recent apps, and shows the page's place black there.
 * Other pages of the app, where this is not shown, can be captured. That is the system's basic way, and the flag is
 * the whole window's, so it is on for as long as the page is shown and no longer.
 */
@Composable
internal fun SecureWindow() {
    val context = LocalContext.current
    DisposableEffect(context) {
        val window =
            generateSequence(context) { (it as? ContextWrapper)?.baseContext }
                .filterIsInstance<Activity>()
                .firstOrNull()
                ?.window
        window?.addFlags(FLAG_SECURE)
        onDispose { window?.clearFlags(FLAG_SECURE) }
    }
}
