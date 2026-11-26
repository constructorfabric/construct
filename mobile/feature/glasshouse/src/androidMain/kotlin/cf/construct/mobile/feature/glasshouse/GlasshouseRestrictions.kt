package cf.construct.mobile.feature.glasshouse

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** An action restricted mode refuses on the Glasshouse page, and says so in one line. */
enum class GlasshouseBlockedAction(
    internal val wire: String,
) {
    Download("download"),
    Print("print"),
    Share("share"),
}

/** One refusal. Each is a new object, so the same action refused again shows its line again. */
class GlasshouseBlocked internal constructor(
    val action: GlasshouseBlockedAction,
)

/**
 * What restricted mode takes away from a page's own commands. A page's print and share are its script's calls, so the
 * page's script is what refuses them: this script runs in every frame before the page's own, tells the app, and
 * rejects a share's promise as a person who says no would.
 */
internal object GlasshouseRestrictions {
    /** The object the script posts to; the app listens to it. */
    const val BRIDGE = "glasshouseBlocked"

    const val SCRIPT = """
(function () {
  var tell = function (action) { $BRIDGE.postMessage(action); };
  var define = function (target, name, value) {
    Object.defineProperty(target, name, {value: value, configurable: true, writable: true});
  };
  define(window, 'print', function () { tell('print'); });
  define(Navigator.prototype, 'share', function () {
    tell('share');
    return Promise.reject(new DOMException('Sharing is blocked in restricted mode.', 'NotAllowedError'));
  });
  define(Navigator.prototype, 'canShare', function () { return false; });
})();
"""

    /** The action the script named, or null for anything else a page might post to the object. */
    fun actionIn(message: String?): GlasshouseBlockedAction? =
        GlasshouseBlockedAction.entries.firstOrNull { it.wire == message }
}

/**
 * What restricted mode keeps of the page while the app runs: the last thing it refused, which the screen shows for a
 * few seconds, and the clipboard [GlasshouseClipboard] takes the page's text off. The page is always in restricted
 * mode and nothing here switches it off.
 */
class GlasshouseRestriction internal constructor(
    context: Context,
    pageHasFocus: () -> Boolean,
) {
    private val clipboard = GlasshouseClipboard(context, pageHasFocus)
    private val mutableBlocked = MutableStateFlow<GlasshouseBlocked?>(null)

    /** The last thing restricted mode refused, until [dismiss] takes it away. */
    val blocked: StateFlow<GlasshouseBlocked?> = mutableBlocked.asStateFlow()

    init {
        clipboard.start()
    }

    internal fun block(action: GlasshouseBlockedAction) {
        mutableBlocked.value = GlasshouseBlocked(action)
    }

    /** Takes [shown] away once it has been shown, unless a newer refusal has come since. */
    fun dismiss(shown: GlasshouseBlocked) {
        mutableBlocked.compareAndSet(shown, null)
    }

    /** Keeps what is on the clipboard now. Call it when the app comes to the front. */
    fun rememberClipboard() {
        clipboard.remember()
    }

    /** Takes what the page put on the clipboard off it. Call it when the app goes to the background. */
    fun restoreClipboard() {
        clipboard.restore()
    }

    internal fun stop() {
        clipboard.stop()
    }
}
