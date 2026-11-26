package cf.construct.mobile.android

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Build
import android.view.accessibility.AccessibilityNodeInfo
import android.webkit.WebView
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.lifecycle.Lifecycle
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.feature.glasshouse.RESTRICTED_TAG
import org.junit.After
import org.junit.Assume.assumeNotNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val PAGE_CODE = "page-secret-code"
private const val KEPT_TEXT = "the person's own text"
private const val NOTICE_GONE_MS = 20_000L
private const val MENU_WAIT_MS = 10_000L
private const val AWAY_WAIT_MS = 10_000L

// The flags of `am start` that bring the app's open window to the front: a new task, and reorder to front.
private const val BRING_TO_FRONT = "0x10020000"
private val SAVE_OR_SHARE =
    listOf("copy", "cut", "share", "save image", "download image", "share image", "read aloud", "web search")

/**
 * Restricted mode on the Glasshouse page, on a device, against the test server of the computer that runs the tests
 * (`scripts/test-glasshouse.py`): a window the phone keeps out of screenshots, a touch menu with no way to copy, cut or
 * share, the page's own copy taken off the clipboard when the app goes away, and the refusals of a download, a print
 * and a share. Touches are real: a finger on the screen, held for a long press.
 */
class GlasshouseRestrictedTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val server = TestServer.fromArguments()
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var page: PageDriver

    @Before
    fun startOnTheChatPage() {
        assumeNotNull("the runner starts the test server and gives its addresses", server)
        page = PageDriver(compose)
        page.forgetCookies()
        server?.reset()
    }

    @After
    fun putTheClipboardBack() {
        if (::page.isInitialized) setClip("")
    }

    private val site get() = checkNotNull(server)

    private fun openTheTestPage() {
        page.openGlasshouse()
        page.go(site.trusted + "/restricted")
        page.waitForPage("Restricted notes")
    }

    private fun statusOfPage() = page.evaluate("document.getElementById('status').textContent")

    // Red when the window is not secure on the page, or stays secure on the other pages, or when the sign is missing
    // from the first moment, before any address is typed.
    @Test
    fun theWindowIsSecureOnThePageAndOnlyThere() {
        for (other in listOf("Chat", "Memory", "Dashboard", "Settings")) {
            page.openTab(other)
            compose.waitForIdle()
            assertFalse(page.secure(), "the window is secure on the $other page")
        }
        page.openGlasshouse()
        compose.waitForIdle()
        assertTrue(page.secure(), "the window is not secure on the Glasshouse page before an address is typed")
        compose.onNodeWithTag(RESTRICTED_TAG).assertIsDisplayed()
        compose.onNodeWithText("Restricted mode").assertIsDisplayed()

        page.go(site.trusted + "/restricted")
        page.waitForPage("Restricted notes")
        assertTrue(page.secure(), "the window is not secure while a page is shown")
        compose.onNodeWithTag(RESTRICTED_TAG).assertIsDisplayed()

        for (other in listOf("Settings", "Chat")) {
            page.openTab(other)
            compose.waitForIdle()
            assertFalse(page.secure(), "the window stays secure on the $other page")
        }
        page.openGlasshouse()
        compose.waitForIdle()
        assertTrue(page.secure(), "the window is not secure when the page is shown again")
    }

    // Red when a screenshot shows the page. The emulator's screenshot, like a person's, leaves a secure window out, so
    // the place of the page is black on the page and has the app's colours on another page.
    @Test
    fun aScreenshotOfThePageShowsNothingOfIt() {
        openTheTestPage()
        val web = checkNotNull(page.webView())
        val area = IntArray(2)
        compose.runOnUiThread { web.getLocationOnScreen(area) }
        val box = listOf(area[0], area[1], web.width, web.height)

        val onThePage = checkNotNull(page.screenshot(secureShown = true)) { "the system gave no screenshot" }
        assertEquals(1, page.coloursIn(onThePage, box), "a screenshot of the page shows something of it")
        assertEquals(Color.BLACK, onThePage.getPixel(box[0] + box[2] / 2, box[1] + box[3] / 2))

        // The window draws its new content a moment after the tab changes: the screenshot is asked for until it does.
        page.openTab("Settings")
        page.waitUntil(PAGE_WAIT_MS, "a screenshot of another page to show it") {
            val elsewhere = page.screenshot(secureShown = true)
            elsewhere != null && page.coloursIn(elsewhere, box) > 1
        }
    }

    // Red when the touch menu on a selected word of the page offers to copy or share it.
    @Test
    fun theTouchMenuOnTextOffersNoCopyAndNoShare() {
        openTheTestPage()
        page.longPressPage("#words")
        val items = waitForMenu("Select all")
        assertTrue(page.evaluate("getSelection().toString()").isNotBlank(), "the long press selected no text")
        assertNothingToCopyOrShare(items)
    }

    // Red when the touch menu in a text field, with text selected in it, offers to cut or copy it; pasting stays. The
    // web view's own menu for selected text in a field has Cut and Copy, so they are what the filter takes away.
    @Test
    fun theTouchMenuInAFieldOffersPasteButNoCutAndNoCopy() {
        openTheTestPage()
        setClip("something to paste")
        page.evaluate(
            "var f = document.getElementById('field'); f.value = 'basil '.repeat(30); f.focus();" +
                "f.setSelectionRange(0, 5); 'set'",
        )
        page.longPressPage("#field")
        val items = waitForMenu("Paste")
        assertEquals(
            "true",
            page.evaluate("var f = document.getElementById('field'); String(f.selectionEnd > f.selectionStart)"),
            "no text is selected in the field",
        )
        assertNothingToCopyOrShare(items)
    }

    // Red when a long press on the picture offers to save, copy or share it. The web view has no menu of its own for a
    // picture, so this stays green with or without the filter; it is red if the web view is ever given one.
    @Test
    fun aLongPressOnAPictureOffersNoWayToSaveOrShareIt() {
        openTheTestPage()
        page.longPressPage("#pic")
        compose.waitForIdle()
        var hit = WebView.HitTestResult.UNKNOWN_TYPE
        compose.runOnUiThread { hit = checkNotNull(page.webView()).hitTestResult.type }
        assertEquals(WebView.HitTestResult.IMAGE_TYPE, hit, "the long press did not land on the picture")
        assertNothingToCopyOrShare(menuItems())
        assertEquals(page.appId, page.foregroundPackage())
    }

    // Red when the text the page's own command puts on the clipboard stays there once the app is in the background, or
    // when what the person had there before does not come back.
    @Test
    fun theTextThePageCopiesIsGoneAfterTheAppGoesToTheBackgroundAndWhatWasThereIsBack() {
        setClip(KEPT_TEXT)
        openTheTestPage()
        page.tapPage("#copy")
        page.waitUntil(PAGE_WAIT_MS, "the page to copy") { statusOfPage() == "copied" }
        assertEquals(PAGE_CODE, clipText(), "the page's own command did not reach the clipboard")

        goToTheBackgroundAndBack()
        assertEquals(KEPT_TEXT, clipText())
    }

    // Red when the page's text stays on a clipboard that had nothing on it.
    @SdkSuppress(minSdkVersion = Build.VERSION_CODES.P)
    @Test
    fun theClipboardIsEmptyAgainWhenItHadNothingBeforeThePageCopied() {
        compose.runOnUiThread { clipboard().clearPrimaryClip() }
        openTheTestPage()
        page.tapPage("#copy")
        page.waitUntil(PAGE_WAIT_MS, "the page to copy") { statusOfPage() == "copied" }
        assertEquals(PAGE_CODE, clipText())

        goToTheBackgroundAndBack()
        assertNull(clipText(), "the clipboard still has \"${clipText()}\"")
    }

    // Red when text the person copied outside the page is taken away with the page's.
    @Test
    fun whatThePersonCopiesWhileTheAppIsOpenIsKept() {
        openTheTestPage()
        page.tapPage("#copy")
        page.waitUntil(PAGE_WAIT_MS, "the page to copy") { statusOfPage() == "copied" }
        page.startTyping()
        setClip(KEPT_TEXT)
        compose.waitForIdle()

        goToTheBackgroundAndBack()
        assertEquals(KEPT_TEXT, clipText())
    }

    // Red when the clipboard is read as the app comes to the front, before its window has the focus: the phone gives
    // nothing then, and the text the person copied while the app was away is lost instead of given back.
    @Test
    fun whatWasCopiedWhileTheAppWasAwayIsBackAfterTheHomeScreen() {
        setClip("copied long before")
        openTheTestPage()
        pressHome()
        setClip(KEPT_TEXT)
        comeBack()
        page.tapPage("#copy")
        page.waitUntil(PAGE_WAIT_MS, "the page to copy") { statusOfPage() == "copied" }
        assertEquals(PAGE_CODE, clipText())

        pressHome()
        comeBack()
        assertEquals(KEPT_TEXT, clipText())
    }

    // Red when a copy made in another app, after the page has copied and while this app has no window focus, is taken
    // for the page's, so the text from before is put over it. The phone tells the app nothing of a copy it cannot read.
    @Test
    fun aCopyInAnotherAppAfterThePageCopiedIsKept() {
        setClip("copied long before")
        openTheTestPage()
        page.tapPage("#copy")
        page.waitUntil(PAGE_WAIT_MS, "the page to copy") { statusOfPage() == "copied" }
        page.shell("cmd statusbar expand-notifications")
        waitForWindowFocus(false)
        setClip(KEPT_TEXT)
        page.shell("cmd statusbar collapse")
        waitForWindowFocus(true)

        goToTheBackgroundAndBack()
        assertEquals(KEPT_TEXT, clipText())
    }

    @Test
    fun aDownloadLinkIsRefusedWithOneLine() {
        openTheTestPage()
        page.tapPage("#download")
        page.waitForBlocked("Download is blocked in restricted mode.")
        assertTrue(page.pageText().contains("Restricted notes"), "the page went away")
        assertFalse("notes.txt" in page.shell("ls /sdcard/Download"), "a file was saved")
    }

    @Test
    fun aFileTheServerSendsToSaveIsRefusedWithOneLine() {
        openTheTestPage()
        page.tapPage("#attachment")
        page.waitForBlocked("Download is blocked in restricted mode.")
        assertTrue(page.pageText().contains("Restricted notes"), "the page went away")
        assertFalse("notes.txt" in page.shell("ls /sdcard/Download"), "a file was saved")
    }

    @Test
    fun printIsRefusedWithOneLineAndNoPrintDialogComesUp() {
        openTheTestPage()
        page.tapPage("#print")
        page.waitForBlocked("Print is blocked in restricted mode.")
        assertEquals("print asked", statusOfPage())
        assertEquals(page.appId, page.foregroundPackage(), "something came up over the app")
    }

    @Test
    fun shareIsRefusedWithOneLineAndTheShareSheetDoesNotComeUp() {
        openTheTestPage()
        page.tapPage("#share")
        page.waitForBlocked("Share is blocked in restricted mode.")
        page.waitUntil(PAGE_WAIT_MS, "the page to hear the refusal") {
            statusOfPage() == "share refused: NotAllowedError"
        }
        assertEquals(page.appId, page.foregroundPackage(), "something came up over the app")
        assertEquals("false", page.evaluate("String(navigator.canShare({text: 'a'}))"))
    }

    // Red when a refusal's line stays for good, or when a second refusal of the same kind shows no line again.
    @Test
    fun theLineOfARefusalGoesAfterAFewSecondsAndComesAgainWithTheNextRefusal() {
        openTheTestPage()
        page.tapPage("#print")
        page.waitForBlocked("Print is blocked")
        compose.mainClock.advanceTimeBy(5_000)
        assertTrue(page.blocked().isNotEmpty(), "the line went before five seconds")
        compose.mainClock.advanceTimeBy(2_000)
        page.waitUntil(NOTICE_GONE_MS, "the line to go") { page.blocked().isEmpty() }
        compose.onNodeWithTag(RESTRICTED_TAG).assertIsDisplayed()
        page.tapPage("#print")
        page.waitForBlocked("Print is blocked")
    }

    private fun clipboard() = compose.activity.getSystemService(ClipboardManager::class.java)

    /** What the person has on the clipboard now, read while the app has the focus, as an app may. */
    private fun clipText(): String? {
        var text: String? = null
        compose.runOnUiThread {
            val clip = clipboard().primaryClip?.takeIf { it.itemCount > 0 }
            text = clip?.getItemAt(0)?.coerceToText(compose.activity)?.toString()
        }
        return text
    }

    private fun setClip(text: String) {
        compose.runOnUiThread { clipboard().setPrimaryClip(ClipData.newPlainText("test", text)) }
    }

    /** The app stops, as it does when the person goes to the home screen, and comes back to the front. */
    private fun goToTheBackgroundAndBack() {
        val scenario = compose.activityRule.scenario
        scenario.moveToState(Lifecycle.State.CREATED)
        scenario.moveToState(Lifecycle.State.RESUMED)
        compose.waitForIdle()
    }

    /** The person presses the home button, so the app loses its window's focus and stops, as it really does. */
    private fun pressHome() {
        instrumentation.uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_HOME)
        page.waitUntil(AWAY_WAIT_MS, "the app to stop") {
            var state = Lifecycle.State.RESUMED
            compose.runOnUiThread { state = compose.activity.lifecycle.currentState }
            !state.isAtLeast(Lifecycle.State.STARTED)
        }
    }

    /** The person opens the app again, and its window takes the focus. */
    private fun comeBack() {
        page.shell("am start -n ${page.appId}/${compose.activity.javaClass.name} -f $BRING_TO_FRONT")
        waitForWindowFocus(true)
    }

    private fun waitForWindowFocus(focused: Boolean) {
        page.waitUntil(AWAY_WAIT_MS, "the window to ${if (focused) "get" else "lose"} the focus") {
            var has = !focused
            compose.runOnUiThread { has = compose.activity.hasWindowFocus() }
            has == focused
        }
    }

    /** The texts of the actions in every window on the screen: the touch menu is a window of its own. */
    private fun menuItems(): List<String> {
        val automation = instrumentation.uiAutomation
        automation.serviceInfo =
            automation.serviceInfo.apply { flags = flags or AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS }
        val texts = mutableListOf<String>()

        fun collect(node: AccessibilityNodeInfo) {
            listOfNotNull(node.text, node.contentDescription).forEach { texts += it.toString() }
            for (index in 0 until node.childCount) node.getChild(index)?.let(::collect)
        }
        automation.windows.forEach { window -> window.root?.let(::collect) }
        return texts
    }

    /** Waits for the touch menu to show [item], and gives what every window on the screen has by then. */
    private fun waitForMenu(item: String): List<String> {
        var items = emptyList<String>()
        try {
            page.waitUntil(MENU_WAIT_MS, "the touch menu to show \"$item\"") {
                items = menuItems()
                item in items
            }
        } catch (gaveUp: IllegalStateException) {
            throw IllegalStateException("${gaveUp.message}; the windows had ${items.distinct()}", gaveUp)
        }
        return items
    }

    private fun assertNothingToCopyOrShare(items: List<String>) {
        val found = items.filter { it.trim().lowercase() in SAVE_OR_SHARE }
        assertTrue(found.isEmpty(), "the touch menu offers $found among $items")
    }
}
