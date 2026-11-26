package cf.construct.mobile.feature.glasshouse

import android.content.Context
import android.graphics.Rect
import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.webkit.WebView

/**
 * A web view whose touch menu, the one that comes up on the text a finger holds, offers only what keeps the text where
 * it is: Select all and Paste. Copy, Cut, Share and every item that hands the text to another app are taken out.
 *
 * The web view starts that menu itself, through [startActionMode], so this is where it is caught. Its items carry ids
 * of the web view's own package, which an app cannot name, so they are known by their titles, which are the system's
 * words for the actions; an item whose title is not one of them is taken out too.
 */
internal class RestrictedWebView(
    context: Context,
) : WebView(context) {
    private val allowedTitles =
        listOf(android.R.string.selectAll, android.R.string.paste, android.R.string.paste_as_plain_text)
            .map { context.getString(it).lowercase() }
            .toSet()

    // `startActionMode(callback)` of a view calls this with the primary type, so overriding this one covers both.
    override fun startActionMode(
        callback: ActionMode.Callback?,
        type: Int,
    ): ActionMode? = super.startActionMode(callback?.let { SelectionMenu(it, allowedTitles) }, type)
}

/** The web view's menu callback, with each menu it builds cut down to the items whose titles are [allowedTitles]. */
private class SelectionMenu(
    private val inner: ActionMode.Callback,
    private val allowedTitles: Set<String>,
) : ActionMode.Callback2() {
    override fun onCreateActionMode(
        mode: ActionMode,
        menu: Menu,
    ): Boolean = inner.onCreateActionMode(mode, menu).also { restrict(menu) }

    // The web view builds its menu again here when the selection changes.
    override fun onPrepareActionMode(
        mode: ActionMode,
        menu: Menu,
    ): Boolean = inner.onPrepareActionMode(mode, menu).also { restrict(menu) }

    override fun onActionItemClicked(
        mode: ActionMode,
        item: MenuItem,
    ): Boolean = allowed(item) && inner.onActionItemClicked(mode, item)

    override fun onDestroyActionMode(mode: ActionMode) = inner.onDestroyActionMode(mode)

    // The floating menu is placed by this, so it has to come from the web view's own callback.
    override fun onGetContentRect(
        mode: ActionMode,
        view: View?,
        outRect: Rect,
    ) {
        if (inner is ActionMode.Callback2) {
            inner.onGetContentRect(mode, view, outRect)
        } else {
            super.onGetContentRect(mode, view, outRect)
        }
    }

    private fun allowed(item: MenuItem): Boolean {
        val title = item.title?.toString().orEmpty()
        return title.trim().lowercase() in allowedTitles
    }

    private fun restrict(menu: Menu) {
        val taken = (0 until menu.size()).map(menu::getItem).filterNot(::allowed)
        taken.forEach { menu.removeItem(it.itemId) }
    }
}
