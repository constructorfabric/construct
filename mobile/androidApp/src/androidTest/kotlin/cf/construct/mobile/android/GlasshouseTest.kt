package cf.construct.mobile.android

import android.content.pm.PackageManager
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.feature.glasshouse.BAR_RELOAD_TAG
import cf.construct.mobile.feature.glasshouse.RELOAD_TAG
import org.junit.After
import org.junit.Assume.assumeNotNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private const val NOT_TRUSTED = "The page cannot open: the site's certificate is not trusted."
private const val REFUSED = "The page cannot open: the site refused the connection."
private const val NOT_FOUND = "The page cannot open: no site was found at this address."
private const val NO_NETWORK = "The page cannot open: there is no network connection."
private const val STOPPED = "The page stopped working."
private const val OTHER = "The page cannot open."
private const val NO_START = "No start address is set. Type an address above to open a page."
private const val ONLY_WEB = "Only http and https addresses open here."
private const val NETWORK_WAIT_MS = 30_000L

/**
 * The Glasshouse page of the app, on a device, against the test server of the computer that runs the tests
 * (`scripts/test-glasshouse.py`). Each test opens the real page with the real address bar and looks at the real page.
 */
class GlasshouseTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val server = TestServer.fromArguments()
    private lateinit var page: PageDriver

    @Before
    fun openThePage() {
        assumeNotNull("the runner starts the test server and gives its addresses", server)
        page = PageDriver(compose)
        page.forgetCookies()
        server?.reset()
        page.openGlasshouse()
    }

    @After
    fun putTheNetworkBack() {
        if (::page.isInitialized && !page.hasNetwork()) {
            page.shell("svc wifi enable")
            page.shell("svc data enable")
        }
        server?.flaky(false)
    }

    private val site get() = checkNotNull(server)

    @Test
    fun aTypedAddressOpens() {
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        assertTrue(page.pageText().contains("Read the herb list"))
        // The bar shows the host of the page, not the whole address, and the keyboard has gone.
        compose.onNodeWithText(site.trusted.removePrefix("https://")).assertExists()
        assertFalse(page.typing(), "the address field still has the focus")
        page.waitUntil(PAGE_WAIT_MS, "the keyboard to go") { !page.keyboardShown() }
    }

    @Test
    fun theServerSeesTheTokenOnThePageItsScriptAndItsPicture() {
        val version =
            InstrumentationRegistry.getInstrumentation().targetContext.let {
                it.packageManager.getPackageInfo(it.packageName, 0).versionName
            }
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.waitUntil(PAGE_WAIT_MS, "the script and the picture to be fetched") {
            val paths = site.requests().map { it.path }
            "/garden.js" in paths && "/garden.png" in paths
        }
        val seen = site.requests().filter { it.path in listOf("/", "/garden.js", "/garden.png") }
        assertEquals(setOf("/", "/garden.js", "/garden.png"), seen.map { it.path }.toSet())
        for (request in seen) {
            assertTrue(request.userAgent.endsWith(" Construct/$version"), "${request.path} sent ${request.userAgent}")
            assertEquals(1, Regex("Construct/").findAll(request.userAgent).count(), request.userAgent)
        }
        // The page's own script sees the same.
        assertTrue(page.evaluate("navigator.userAgent").endsWith(" Construct/$version"))
    }

    // Red when the page's web view does not take cookies, or keeps them apart from the app's other web views (a
    // profile of its own, say). That they are not the phone's browser's is the platform's: an app's web views have
    // their own data folder, so no test of the app's code can show otherwise; it was checked by hand against Chrome.
    @Test
    fun theCookiesAreOneSetForEveryWebViewOfTheApp() {
        page.go(site.trusted + "/signin")
        page.waitForPage("Signed in")
        // A second web view of the app, made apart from the page, sends the cookies the page was given.
        val other = arrayOf("")
        compose.runOnUiThread {
            val web = android.webkit.WebView(compose.activity)
            web.settings.javaScriptEnabled = true
            web.webViewClient =
                object : android.webkit.WebViewClient() {
                    override fun onPageFinished(
                        view: android.webkit.WebView,
                        url: String?,
                    ) {
                        view.evaluateJavascript("document.body.innerText") { other[0] = it }
                    }
                }
            web.loadUrl(site.trusted + "/account")
        }
        page.waitUntil(
            PAGE_WAIT_MS,
            "the second web view to read the account page",
        ) { other[0].contains("Signed in as Sam") }
    }

    @Test
    fun aLinkThatAsksForANewWindowOpensInThePage() {
        page.go(site.trusted + "/new-window")
        page.waitForPage("New windows")
        page.tapPage("#blank")
        page.waitForPage("Basil, mint and parsley.")
        assertEquals(1, page.webViewCount())
        page.evaluate("history.back()")
        page.waitForPage("New windows")
        page.tapPage("#open")
        page.waitForPage("Basil, mint and parsley.")
        assertEquals(1, page.webViewCount())
        assertEquals(page.appId, page.foregroundPackage())
    }

    @Test
    fun aLinkToAnAppAddressDoesNothingAndLaunchesNothing() {
        page.go(site.trusted + "/app-links")
        page.waitForPage("App links")
        for ((tapped, link) in listOf("intent", "market", "tel", "mailto", "custom", "redirect").withIndex()) {
            page.tapPage("#$link")
            // The tap did reach the page; what is checked next is what the app did with the link.
            page.waitUntil(PAGE_WAIT_MS, "the tap on the $link link to reach the page") {
                page.evaluate("document.body.getAttribute('data-clicks') || '0'") == "${tapped + 1}"
            }
            // Nothing signals that the app did nothing, so this waits a moment for something to come up and then looks.
            Thread.sleep(1_500)
            assertTrue(page.pageText().contains("App links"), "the page changed after the $link link")
            assertEquals(page.appId, page.foregroundPackage(), "something else came up after the $link link")
            assertEquals("", page.message(), "the page told of a problem after the $link link")
        }
        assertTrue(
            compose.activity.lifecycle.currentState
                .isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED),
        )
    }

    @Test
    fun thePageIsWholeAndWhatWasTypedInItIsStillThereAfterTheChatPage() {
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.tapPage("#note")
        InstrumentationRegistry.getInstrumentation().sendStringSync("basil")
        page.waitUntil(PAGE_WAIT_MS, "the typed text to be in the field") {
            page.evaluate("document.getElementById('note').value") ==
                "basil"
        }
        val webView = page.webView()
        page.openTab("Chat")
        compose.onNodeWithText("What do you know about me?").assertExists()
        page.openGlasshouse()
        page.waitForPage("Garden notes")
        assertEquals("basil", page.evaluate("document.getElementById('note').value"))
        assertEquals("ran", page.evaluate("document.body.getAttribute('data-script')"))
        // It is the same web view, not a page loaded again.
        assertTrue(webView === page.webView())
    }

    @Test
    fun aPageKeepsLoadingWhileThePersonIsOnTheChatPage() {
        page.go(site.trusted + "/slow")
        val web = checkNotNull(page.webView())
        page.openTab("Chat")
        compose.onNodeWithText("What do you know about me?").assertExists()
        // The page is read where it is, out of the screen: it has to finish loading while the person is on the Chat
        // page. A web view that was paused when it left never answers here, and one that was loaded again on the
        // way back would be asked for twice.
        page.waitUntil(PAGE_WAIT_MS, "the slow page to load while the Chat page shows") {
            page.pageText(web).contains("Slow page loaded")
        }
        page.openGlasshouse()
        page.waitForPage("Slow page loaded")
        assertEquals(1, site.requests().count { it.path == "/slow" })
    }

    @Test
    fun anUntrustedCertificateShowsTheErrorAndReloadTriesAgain() {
        page.go(site.untrusted + "/")
        page.waitForMessage(NOT_TRUSTED)
        assertFalse(page.pageText().contains("Garden notes"))
        compose.onNodeWithTag(RELOAD_TAG).assertExists()
        val before = site.connections("untrusted")
        compose.onNodeWithTag(RELOAD_TAG).performClick()
        page.waitUntil(PAGE_WAIT_MS, "Reload to try the site again") { site.connections("untrusted") > before }
        page.waitForMessage(NOT_TRUSTED)
    }

    @Test
    fun anUntrustedCertificateShowsTheErrorForAnAddressTheWebViewWritesAnotherWay() {
        // The web view reports the page as it writes the address, without the dot segments and the fragment.
        page.go(site.untrusted + "/herbs/../#top")
        page.waitForMessage(NOT_TRUSTED)
        compose.onNodeWithTag(RELOAD_TAG).assertExists()
    }

    @Test
    fun anAddressThatCannotBeReachedShowsItsMessageAndReloadTriesAgain() {
        // No site has this name.
        page.go("https://no-such-site.invalid/")
        page.waitForMessage(NOT_FOUND)
        compose.onNodeWithTag(RELOAD_TAG).assertExists()

        // The site refuses until it listens; Reload then opens it.
        site.flaky(false)
        page.go(site.flaky + "/")
        page.waitForMessage(REFUSED)
        site.flaky(true)
        compose.onNodeWithTag(RELOAD_TAG).performClick()
        page.waitForPage("Garden notes")
        assertEquals("", page.message())
    }

    @Test
    fun aPhoneWithNoNetworkShowsItsMessageAndReloadTriesAgain() {
        // A page is open first, so the web view holds a connection to its site that the lost network cuts.
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.shell("svc wifi disable")
        page.shell("svc data disable")
        page.waitUntil(NETWORK_WAIT_MS, "the phone to lose its network") { !page.hasNetwork() }
        page.go(site.trusted + "/herbs")
        page.waitForMessage(NO_NETWORK)

        // Another address says the same, and it is the one Reload opens once the phone has its network back. An
        // emulator whose network was switched leaves the connection of the first site unusable for minutes, so
        // the retry goes to a site the web view holds no connection to.
        page.go(site.flaky + "/")
        page.waitForMessage(NO_NETWORK)
        page.shell("svc wifi enable")
        page.shell("svc data enable")
        page.waitUntil(NETWORK_WAIT_MS, "the phone to get its network back") { page.hasNetwork() }
        // The test server is on the same network: ask it to listen on the second site's port.
        page.waitUntil(
            NETWORK_WAIT_MS,
            "the test server to answer again",
        ) { runCatching { site.flaky(true) }.isSuccess }
        compose.onNodeWithTag(RELOAD_TAG).performClick()
        page.waitForPage("Garden notes")
        assertEquals("", page.message())
    }

    @Test
    fun aPlainHttpAddressIsBlockedAsAPageThatCannotOpen() {
        page.go(site.plain + "/")
        page.waitForMessage(OTHER)
        assertEquals(0, site.requests().count { it.site == "plain" })
    }

    @Test
    fun anAddressThatIsNotAWebAddressIsRefusedAndTheOldPageStays() {
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.go("ftp://example.test/herbs")
        compose.onNodeWithText(ONLY_WEB).assertExists()
        assertTrue(page.pageText().contains("Garden notes"))
    }

    @Test
    fun stoppingTheWebProcessLeavesTheAppRunningWithReload() {
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        val first = checkNotNull(page.webView())
        compose.runOnUiThread { first.loadUrl("chrome://crash") }
        page.waitForMessage(STOPPED)
        assertFalse(compose.activity.isFinishing)
        // The rest of the app works.
        page.openTab("Chat")
        compose.onNodeWithText("What do you know about me?").assertExists()
        page.openGlasshouse()
        page.waitForMessage(STOPPED)
        compose.onNodeWithTag(RELOAD_TAG).performClick()
        page.waitForPage("Garden notes")
        assertTrue(first !== page.webView(), "the page should have a new web view")
    }

    @Test
    fun theSystemBackGoesBackInThePageAndReloadLoadsItAgain() {
        page.go(site.trusted + "/")
        page.waitForPage("Garden notes")
        page.waitUntil(
            PAGE_WAIT_MS,
            "the page to finish loading",
        ) { page.evaluate("document.readyState") == "complete" }
        // A link, as a person follows it: a page opened while the first still loads would replace it in the history.
        page.tapPage("#herbs")
        page.waitForPage("Basil, mint and parsley.")
        // The history has to be in place, and the screen has to have heard of it, before the back is pressed.
        page.waitUntil(PAGE_WAIT_MS, "the page to have a page behind it") {
            var behind = false
            compose.runOnUiThread { behind = checkNotNull(page.webView()).canGoBack() }
            behind
        }
        compose.waitForIdle()
        page.pressBack()
        page.waitForPage("Garden notes")
        assertEquals(page.appId, page.foregroundPackage())

        val loads = site.requests().count { it.path == "/" }
        compose.onNodeWithTag(BAR_RELOAD_TAG).performClick()
        page.waitUntil(
            PAGE_WAIT_MS,
            "the page to be fetched again",
        ) { site.requests().count { it.path == "/" } > loads }
    }

    @Test
    fun noStartAddressShowsTheNotice() {
        compose.onNodeWithText(NO_START).assertExists()
        assertEquals(NO_START, page.message())
    }

    @Test
    fun aRequestForTheCameraOrTheLocationIsRefusedWithNoPrompt() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        // The app holds the camera permission for pairing; give it that too, so only the page's refusal stands in
        // the way.
        page.shell("pm grant ${context.packageName} android.permission.CAMERA")
        page.go(site.trusted + "/permissions")
        page.waitUntil(PAGE_WAIT_MS, "the page to hear back about both") {
            page.evaluate("document.body.getAttribute('data-done') || ''") == "yes"
        }
        val result = page.evaluate("document.getElementById('result').textContent")
        assertTrue(result.contains("camera:NotAllowedError"), result)
        // The app holds no location permission, so the page is denied (code 1) whatever the callback answers: this
        // half shows that no prompt comes up and the page hears no, not that the refusal in the callback is the cause.
        assertTrue(result.contains("location:code1"), result)
        assertEquals(context.packageName, page.foregroundPackage(), "a prompt came up")
    }

    @Test
    fun thePageAddsNoPermissionToTheFourTheEarlierPagesAsk() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val asked =
            context.packageManager
                .getPackageInfo(
                    context.packageName,
                    PackageManager.PackageInfoFlags.of(PackageManager.GET_PERMISSIONS.toLong()),
                ).requestedPermissions
                .orEmpty()
                // The app's own permission for its receivers, which the libraries add, is not one a page could use.
                .filter { it.startsWith("android.permission.") }
                .toSet()
        // The four the app already had before this page: the network, its state, the local network and the camera
        // (the last two are the pairing page's). A page that asks for the microphone, the location or anything else
        // is refused in the web view, and no permission is added for it.
        assertEquals(
            setOf(
                "android.permission.INTERNET",
                "android.permission.ACCESS_NETWORK_STATE",
                "android.permission.ACCESS_LOCAL_NETWORK",
                "android.permission.CAMERA",
            ),
            asked,
        )
    }
}
