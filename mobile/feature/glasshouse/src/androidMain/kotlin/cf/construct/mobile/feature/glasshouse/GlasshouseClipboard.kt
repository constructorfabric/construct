package cf.construct.mobile.feature.glasshouse

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build

/**
 * Takes back what the page puts on the clipboard. The page can write there with its own commands, and the phone has
 * no way to forbid it, so the text is removed when the app goes to the background and what was there before is put
 * back, or nothing if nothing was.
 *
 * A change counts as the page's when the page's web view has the focus and so does the app's window, which they have
 * when a page's command runs; a change made anywhere else, such as a copy in the address bar or in another app beside
 * this one, is the person's and becomes what is kept. The phone lets an app read the clipboard only while its window
 * has the focus, so [remember] is for the moment the window gets it, and [restore] only writes, which needs none.
 */
internal class GlasshouseClipboard(
    context: Context,
    private val pageHasFocus: () -> Boolean,
) {
    private val manager = checkNotNull(context.getSystemService(ClipboardManager::class.java))
    private val listener = ClipboardManager.OnPrimaryClipChangedListener { changed() }
    private var before: ClipData? = null
    private var pageWrote = false
    private var pageClipTime = 0L

    /** Starts watching, and keeps what is on the clipboard now, if the phone lets the app read it. */
    fun start() {
        manager.addPrimaryClipChangedListener(listener)
        remember()
    }

    fun stop() {
        manager.removePrimaryClipChangedListener(listener)
    }

    /**
     * Keeps what is on the clipboard now, as what [restore] puts back, unless the page's text is what is there. Call it
     * when the app's window gets the focus, which is when the phone lets it read the clipboard, and which is how a copy
     * made in another app meanwhile, that the phone did not tell the app of, is found.
     */
    fun remember() {
        val now = manager.primaryClip
        if (pageWrote && (pageClipTime == 0L || now?.description?.timestamp == pageClipTime)) return
        before = now
        pageWrote = false
    }

    /** Takes the page's text off the clipboard, if the page put any there since [remember]. */
    fun restore() {
        if (!pageWrote) return
        pageWrote = false
        if (!put(before)) clear()
    }

    // A clip that came from another app can hold an address this app may not hand on, and the phone refuses to put it
    // back; then the clipboard is cleared, so the page's text is gone in any case.
    private fun put(clip: ClipData?): Boolean {
        if (clip == null) return false
        return try {
            manager.setPrimaryClip(clip)
            true
        } catch (_: SecurityException) {
            false
        }
    }

    private fun clear() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            manager.clearPrimaryClip()
        } else {
            manager.setPrimaryClip(ClipData.newPlainText("", ""))
        }
    }

    private fun changed() {
        if (pageHasFocus()) {
            pageWrote = true
            pageClipTime = manager.primaryClip?.description?.timestamp ?: 0L
        } else {
            remember()
        }
    }
}
