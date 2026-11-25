package cf.construct.mobile.android

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebView
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextReplacement
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.feature.glasshouse.ADDRESS_TAG
import cf.construct.mobile.feature.glasshouse.MESSAGE_TAG
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

internal const val PAGE_WAIT_MS = 40_000L
private const val POLL_MS = 250L
private const val JS_WAIT_SECONDS = 10L
private const val FOCUS_WAIT_MS = 10_000L

/** One request the test server was asked for, as it saw it. */
internal data class ServedRequest(
    val site: String,
    val path: String,
    val userAgent: String,
    val cookie: Boolean,
)

/**
 * The test server on the computer that runs the tests (`scripts/test-glasshouse.py` starts it and passes the addresses
 * as instrumentation arguments). The debug build trusts its certificate, so the app's own connection reaches it.
 */
internal class TestServer(
    val trusted: String,
    val untrusted: String,
    val plain: String,
    val flaky: String,
) {
    private fun call(path: String): String {
        val connection = URL(trusted + path).openConnection() as HttpURLConnection
        connection.connectTimeout = 10_000
        connection.readTimeout = 10_000
        return connection.inputStream.use { it.readBytes().decodeToString() }
    }

    fun requests(): List<ServedRequest> {
        val list = JSONArray(call("/__requests"))
        return (0 until list.length()).map {
            val one = list.getJSONObject(it)
            ServedRequest(
                one.getString("site"),
                one.getString("path"),
                one.getString("userAgent"),
                one.getBoolean("cookie"),
            )
        }
    }

    fun reset() {
        call("/__reset")
    }

    /** How many connections the site with this name was asked for since the last [reset]. */
    fun connections(site: String): Int = JSONObject(call("/__counts")).optInt(site, 0)

    /** Opens or closes the `flaky` port. */
    fun flaky(on: Boolean) {
        call("/__flaky?on=${if (on) 1 else 0}")
    }

    companion object {
        /**
         * The server of this run, or null when the run did not start one (a plain `connectedCheck`, whose tests then
         * skip themselves). The runner marks its runs with `glasshouseRunner`: such a run with no server is an error,
         * not a run of skipped tests that reports OK.
         */
        fun fromArguments(): TestServer? {
            val arguments = InstrumentationRegistry.getArguments()
            val trusted = arguments.getString("glasshouseTrusted")
            if (trusted == null) {
                check(arguments.getString("glasshouseRunner") == null) { "the runner gave no test server" }
                return null
            }
            return TestServer(
                trusted,
                arguments.getString("glasshouseUntrusted").orEmpty(),
                arguments.getString("glasshousePlain").orEmpty(),
                arguments.getString("glasshouseFlaky").orEmpty(),
            )
        }
    }
}

/** Drives the app's real Glasshouse page the way a person does, and reads the page the way a person sees it. */
internal class PageDriver(
    private val compose: AndroidComposeTestRule<ActivityScenarioRule<MainActivity>, MainActivity>,
) {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    val appId: String get() = instrumentation.targetContext.packageName

    fun openTab(name: String) {
        compose.onNodeWithText(name).performClick()
    }

    fun openGlasshouse() = openTab("Glasshouse")

    /** Whether the address field has the focus: the person is typing. */
    fun typing(): Boolean =
        compose.onAllNodes(hasTestTag(ADDRESS_TAG) and isFocused()).fetchSemanticsNodes().isNotEmpty()

    /** Whether the keyboard is on the screen. */
    fun keyboardShown(): Boolean {
        var shown = false
        compose.runOnUiThread {
            shown =
                ViewCompat
                    .getRootWindowInsets(
                        compose.activity.window.decorView,
                    )?.isVisible(WindowInsetsCompat.Type.ime()) ==
                true
        }
        return shown
    }

    /** Taps the address bar and waits for the text field that appears under the tap to take the focus. */
    fun startTyping() {
        compose.onNodeWithTag(ADDRESS_TAG).performClick()
        waitUntil(FOCUS_WAIT_MS, "the address field to take the focus") { typing() }
    }

    /** Taps the address bar, types [address] into it and presses Go. */
    fun go(address: String) {
        startTyping()
        compose.onNodeWithTag(ADDRESS_TAG).performTextReplacement(address)
        compose.onNodeWithTag(ADDRESS_TAG).performImeAction()
    }

    private fun webViewIn(view: View): WebView? =
        when (view) {
            is WebView -> view
            is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { webViewIn(view.getChildAt(it)) }
            else -> null
        }

    /** The web view of the page, or null before the person has opened it. */
    fun webView(): WebView? {
        var found: WebView? = null
        compose.runOnUiThread { found = webViewIn(compose.activity.window.decorView) }
        return found
    }

    fun webViewCount(): Int {
        fun count(view: View): Int =
            if (view is WebView) {
                1
            } else if (view is ViewGroup) {
                (0 until view.childCount).sumOf { count(view.getChildAt(it)) }
            } else {
                0
            }
        var total = 0
        compose.runOnUiThread { total = count(compose.activity.window.decorView) }
        return total
    }

    /**
     * What the page's script says [script] is; the page's own text, JSON-decoded. It asks the page on the screen, or
     * the web view [on] when the page is not on the screen (the person is on another page of the app).
     */
    fun evaluate(
        script: String,
        on: WebView? = webView(),
    ): String {
        val web = checkNotNull(on) { "the page has no web view yet" }
        val done = CountDownLatch(1)
        var result = ""
        compose.runOnUiThread {
            web.evaluateJavascript(script) {
                result = it
                done.countDown()
            }
        }
        check(done.await(JS_WAIT_SECONDS, TimeUnit.SECONDS)) { "the page did not answer a script" }
        return (JSONTokener(result).nextValue() as? String) ?: result
    }

    private var lastFailure: Throwable? = null

    /** The text of the page, or an empty text while there is none. */
    fun pageText(on: WebView? = webView()): String =
        runCatching { evaluate("document.body ? document.body.innerText : ''", on) }
            .onFailure { lastFailure = it }
            .getOrDefault("")

    fun waitForPage(
        text: String,
        timeoutMs: Long = PAGE_WAIT_MS,
    ) {
        waitUntil(timeoutMs, "the page to say \"$text\"") { pageText().contains(text) }
    }

    /** Waits until the page's message (error or notice) contains [text]. */
    fun waitForMessage(
        text: String,
        timeoutMs: Long = PAGE_WAIT_MS,
    ) {
        waitUntil(timeoutMs, "the message \"$text\"") { message().contains(text) }
    }

    /** The text of the page's message, or an empty text when it shows none. */
    fun message(): String =
        compose
            .onAllNodesWithTag(MESSAGE_TAG)
            .fetchSemanticsNodes()
            .firstOrNull()
            ?.config
            ?.getOrNull(SemanticsProperties.Text)
            ?.joinToString("") { it.text }
            .orEmpty()

    fun waitUntil(
        timeoutMs: Long,
        what: String,
        condition: () -> Boolean,
    ) {
        val end = SystemClock.uptimeMillis() + timeoutMs
        while (SystemClock.uptimeMillis() < end) {
            // The screen gets to draw what the state says before anything is looked at or tapped.
            compose.waitForIdle()
            if (condition()) return
            Thread.sleep(POLL_MS)
        }
        snapshot("failed")
        error(
            "gave up waiting for $what; the page said \"${pageText().take(120)}\", the message \"${message()}\"" +
                (lastFailure?.let { ", the last script failed with $it" } ?: "") + ", the last tap $lastTap",
        )
    }

    /** Taps the page element that matches [selector] the way a person's finger does: a touch of the screen. */
    fun tapPage(selector: String) {
        compose.waitForIdle()
        val web = checkNotNull(webView())
        val box =
            JSONObject(
                evaluate(
                    "(function(){var r=document.querySelector('$selector').getBoundingClientRect();" +
                        "return JSON.stringify({x:(r.left+r.right)/2,y:(r.top+r.bottom)/2," +
                        "d:window.devicePixelRatio});})()",
                ),
            )
        val origin = IntArray(2)
        compose.runOnUiThread { web.getLocationOnScreen(origin) }
        val x = (origin[0] + box.getDouble("x") * box.getDouble("d")).roundToInt()
        val y = (origin[1] + box.getDouble("y") * box.getDouble("d")).roundToInt()
        lastTap = "$selector at $x,$y (the page at ${origin[0]},${origin[1]}, ratio ${box.getDouble("d")})"
        shell("input tap $x $y")
    }

    private var lastTap = "none"

    /** Saves a picture of the screen for whoever reads a failure; `adb pull` it from the app's folder. */
    fun snapshot(name: String) {
        val folder =
            File(
                checkNotNull(instrumentation.targetContext.getExternalFilesDir(null)),
                "glasshouse-shots",
            ).apply {
                mkdirs()
            }
        val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: return
        File(folder, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    /**
     * The package of the window that has the focus, which is the app's while the person is in it, and another
     * app's or the system's when something has come up over it. Waits a moment while no window has it.
     */
    fun foregroundPackage(): String {
        val end = SystemClock.uptimeMillis() + FOCUS_WAIT_MS
        while (true) {
            val line = shell("dumpsys window").lineSequence().firstOrNull { "mCurrentFocus=" in it }.orEmpty()
            val found = Regex("""\bu\d+ ([^/}\s]+)""").find(line)?.groupValues?.get(1)
            if (found != null || SystemClock.uptimeMillis() > end) return found.orEmpty()
            Thread.sleep(POLL_MS)
        }
    }

    /** Presses the system's back, the way the back button or gesture does, and waits for the app to deal with it. */
    fun pressBack() {
        instrumentation.uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)
        instrumentation.waitForIdleSync()
    }

    /** Runs a shell command as the shell user and waits for it to end. */
    fun shell(command: String): String {
        val descriptor = instrumentation.uiAutomation.executeShellCommand(command)
        return android.os.ParcelFileDescriptor
            .AutoCloseInputStream(descriptor)
            .use { it.readBytes().decodeToString() }
    }

    fun hasNetwork(): Boolean {
        val manager = instrumentation.targetContext.getSystemService(android.net.ConnectivityManager::class.java)
        return manager.activeNetwork != null
    }

    fun forgetCookies() {
        val done = CountDownLatch(1)
        compose.runOnUiThread { CookieManager.getInstance().removeAllCookies { done.countDown() } }
        done.await(JS_WAIT_SECONDS, TimeUnit.SECONDS)
    }
}
