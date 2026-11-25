package cf.construct.mobile.android

import android.graphics.Bitmap
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTextReplacement
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.feature.glasshouse.ADDRESS_TAG
import org.junit.Assume.assumeNotNull
import org.junit.Rule
import org.junit.Test
import java.io.File

/**
 * Takes a screenshot of the page in each of its states, for the pull request. The runner sets the theme, passes its
 * name as `glasshouseTheme`, and pulls the files from the app's folder; a run with no such argument skips it.
 */
class GlasshouseShotsTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val theme = InstrumentationRegistry.getArguments().getString("glasshouseTheme")
    private val server = TestServer.fromArguments()

    private lateinit var page: PageDriver

    /** Takes the picture once the screen has drawn what the state says; the keyboard is gone unless it is the point. */
    private fun shot(
        state: String,
        keyboard: Boolean = false,
    ) {
        compose.waitForIdle()
        if (!keyboard) page.waitUntil(KEYBOARD_WAIT, "the keyboard to go") { !page.keyboardShown() }
        compose.waitForIdle()
        // The emulator draws slowly: let the frame that shows this state reach the screen.
        Thread.sleep(FRAME_PAUSE_MS)
        val folder =
            File(
                checkNotNull(instrumentation.targetContext.getExternalFilesDir(null)),
                "glasshouse-shots",
            ).apply {
                mkdirs()
            }
        val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
        File(folder, "$theme-$state.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    @Test
    fun everyState() {
        assumeNotNull("only the screenshot run sets these", theme, server)
        val site = checkNotNull(server)
        page = PageDriver(compose)
        page.forgetCookies()
        page.openGlasshouse()
        compose.waitForIdle()
        shot("nostart")

        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.waitUntil(
            PAGE_WAIT_MS,
            "the page to finish loading",
        ) { page.evaluate("document.readyState") == "complete" }
        Thread.sleep(800)
        shot("ready")

        // An address that is not a web one is refused under the bar, and the page stays where it was.
        page.go("ftp://example.test/herbs")
        shot("refused", keyboard = true)

        page.startTyping()
        compose.onNodeWithTag(ADDRESS_TAG).performTextReplacement(site.trusted + "/")
        shot("typing", keyboard = true)

        page.go(site.trusted + "/slow")
        shot("loading")
        page.waitForPage("Slow page loaded")

        page.go("https://no-such-site.invalid/")
        page.waitForMessage("no site was found")
        shot("notfound")

        page.go(site.untrusted + "/")
        page.waitForMessage("certificate is not trusted")
        shot("certificate")

        site.flaky(false)
        page.go(site.flaky + "/")
        page.waitForMessage("refused the connection")
        shot("connection-refused")

        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        compose.runOnUiThread { checkNotNull(page.webView()).loadUrl("chrome://crash") }
        page.waitForMessage("The page stopped working.")
        shot("stopped")

        page.shell("svc wifi disable")
        page.shell("svc data disable")
        try {
            page.waitUntil(NETWORK_WAIT, "the phone to lose its network") { !page.hasNetwork() }
            page.go(site.trusted + "/")
            page.waitForMessage("no network connection")
            shot("offline")
        } finally {
            page.shell("svc wifi enable")
            page.shell("svc data enable")
        }
    }
}

private const val NETWORK_WAIT = 30_000L
private const val KEYBOARD_WAIT = 10_000L
private const val FRAME_PAUSE_MS = 700L
