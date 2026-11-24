package cf.construct.mobile.feature.glasshouse

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val START = "https://example.org/start"

/** A holder and a web view that collects its commands, as the phone's page does. */
@OptIn(ExperimentalCoroutinesApi::class)
private class Page(
    private val scope: TestScope,
    configured: String? = START,
) {
    val holder = GlasshouseHolder(configured)
    private val received = mutableListOf<GlasshouseCommand>()
    private var webView: Job? = null

    val state get() = holder.state.value

    init {
        startWebView()
    }

    /** A web view starts: it says so and collects the commands from now on. */
    fun startWebView() {
        holder.webViewStarted()
        collect()
    }

    /** The same web view collects again, as a screen that redraws does. */
    fun collectAgain() {
        stopWebView()
        collect()
    }

    /** The web view goes away. */
    fun stopWebView() {
        webView?.cancel()
        scope.runCurrent()
    }

    private fun collect() {
        webView =
            scope.backgroundScope.launch(UnconfinedTestDispatcher(scope.testScheduler)) {
                holder.commands.toList(received)
            }
        scope.runCurrent()
    }

    /** The commands the web view got since the last call. */
    fun sent(): List<GlasshouseCommand> {
        scope.runCurrent()
        return received.toList().also { received.clear() }
    }

    /** The start address has loaded, with the history the test asks for. */
    fun loaded(
        canGoBack: Boolean = false,
        canGoForward: Boolean = false,
    ): Page {
        sent()
        holder.pageStarted(START)
        holder.navigated(START, canGoBack, canGoForward)
        holder.pageFinished()
        return this
    }
}

class GlasshouseHolderTest {
    @Test
    fun beforeAWebViewStartsTheStateHoldsTheStartAddressAndNothingLoads() {
        assertEquals(GlasshouseState(address = START), GlasshouseHolder(START).state.value)
        assertEquals(GlasshouseState(address = "https://example.org"), GlasshouseHolder("example.org").state.value)
    }

    @Test
    fun theWebViewIsToldToOpenTheStartAddress() =
        runTest {
            val page = Page(this)

            assertEquals(GlasshouseState(address = START, loading = true), page.state)
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open(START)), page.sent())
        }

    @Test
    fun aStartAddressWithNoSchemeOpensOverHttps() =
        runTest {
            val page = Page(this, "example.org")

            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open("https://example.org")), page.sent())
        }

    @Test
    fun withNoStartAddressTheStateSaysSoAndNothingOpens() =
        runTest {
            for (configured in listOf(null, "", "  ", "file:///etc/passwd", "not an address")) {
                val page = Page(this, configured)

                assertEquals(GlasshouseState(noStartAddress = true), page.state, "$configured")
                assertEquals(emptyList<GlasshouseCommand>(), page.sent(), "$configured")
            }
        }

    @Test
    fun theStateAfterTheStartAddressFollowsTheWebView() =
        runTest {
            val page = Page(this)
            page.sent()

            page.holder.pageStarted(START)
            assertTrue(page.state.loading)
            page.holder.navigated(START, canGoBack = false, canGoForward = false)
            page.holder.pageFinished()

            assertEquals(GlasshouseState(address = START), page.state)
        }

    @Test
    fun anAddressTypedWithNoSchemeOpensOverHttps() =
        runTest {
            val page = Page(this, null)

            page.holder.go("example.com/docs")

            assertEquals(
                GlasshouseState(address = "https://example.com/docs", loading = true, noStartAddress = true),
                page.state,
            )
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open("https://example.com/docs")), page.sent())
        }

    @Test
    fun anAddressTypedWithASchemeOpensAsTyped() =
        runTest {
            val page = Page(this).loaded()

            page.holder.go("http://example.com:8080/x")

            assertEquals("http://example.com:8080/x", page.state.address)
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open("http://example.com:8080/x")), page.sent())
        }

    @Test
    fun anAddressOfAnotherKindIsRefusedAndNothingOpens() =
        runTest {
            val page = Page(this).loaded()
            val before = page.state

            val refused = listOf("file:///etc/passwd", "javascript:alert(1)", "data:text/html,hi", "mailto:a@b.c", "")
            for (text in refused + "hello world") {
                page.holder.go(text)

                assertEquals(before.copy(addressRefused = true), page.state, text)
            }
            assertEquals(emptyList<GlasshouseCommand>(), page.sent())
        }

    @Test
    fun aRefusalGoesWithAnyLaterAction() =
        runTest {
            val actions =
                mapOf<String, (GlasshouseHolder) -> Unit>(
                    "edit" to { it.addressEdited() },
                    "go" to { it.go("example.com") },
                    "back" to { it.back() },
                    "forward" to { it.forward() },
                    "reload" to { it.reload() },
                )
            for ((name, action) in actions) {
                val page = Page(this).loaded(canGoBack = true, canGoForward = true)
                page.holder.go("file:///etc/passwd")
                assertTrue(page.state.addressRefused, name)

                action(page.holder)

                assertFalse(page.state.addressRefused, name)
            }
        }

    @Test
    fun backAndForwardAreSentOnlyWhenThereIsAPageToGoTo() =
        runTest {
            val page = Page(this).loaded()

            page.holder.back()
            page.holder.forward()
            assertEquals(emptyList<GlasshouseCommand>(), page.sent())

            page.holder.navigated(START, canGoBack = true, canGoForward = false)
            page.holder.back()
            page.holder.forward()
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Back), page.sent())

            page.holder.navigated(START, canGoBack = true, canGoForward = true)
            page.holder.forward()
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Forward), page.sent())
        }

    @Test
    fun theHistoryAndTheAddressFollowTheWebView() =
        runTest {
            val page = Page(this).loaded()

            page.holder.go("example.com")
            page.holder.pageStarted("https://example.com")
            page.holder.navigated("https://example.com", canGoBack = true, canGoForward = false)
            page.holder.pageFinished()
            assertEquals(GlasshouseState(address = "https://example.com", canGoBack = true), page.state)

            page.holder.back()
            page.holder.pageStarted(START)
            page.holder.navigated(START, canGoBack = false, canGoForward = true)
            page.holder.pageFinished()
            assertEquals(GlasshouseState(address = START, canGoForward = true), page.state)
        }

    @Test
    fun aRedirectOrALinkShowsItsAddress() =
        runTest {
            val page = Page(this).loaded()

            page.holder.pageStarted("https://example.org/moved")
            assertEquals("https://example.org/moved", page.state.address)
            page.holder.navigated("https://example.org/moved#part", canGoBack = true, canGoForward = false)
            assertEquals("https://example.org/moved#part", page.state.address)
        }

    @Test
    fun aFailureShowsTheAddressThatFailed() =
        runTest {
            val page = Page(this).loaded()

            page.holder.pageFailed("https://third.example/", GlasshouseProblem.NotFound)

            assertEquals("https://third.example/", page.state.address)
        }

    @Test
    fun aReportAboutAnAddressThatIsNotWebChangesNothing() =
        runTest {
            val page = Page(this).loaded(canGoBack = true)
            page.holder.pageFailed(START, GlasshouseProblem.Offline)
            val before = page.state

            for (reported in listOf("about:blank", "data:text/html,chromewebdata", "file:///x", "")) {
                page.holder.pageStarted(reported)
                page.holder.navigated(reported, canGoBack = false, canGoForward = false)

                assertEquals(before, page.state, reported)
            }
        }

    @Test
    fun aFailureAtAnAddressThatIsNotWebStillShowsAndKeepsTheLastWebAddress() =
        runTest {
            val page = Page(this).loaded()

            page.holder.pageFailed("about:blank", GlasshouseProblem.Other)

            assertEquals(GlasshouseProblem.Other, page.state.problem)
            assertEquals(START, page.state.address)
        }

    @Test
    fun reloadLoadsThePageAgain() =
        runTest {
            val page = Page(this).loaded()

            page.holder.reload()

            assertTrue(page.state.loading)
            assertNull(page.state.problem)
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Reload), page.sent())
        }

    @Test
    fun reloadWithNoPageDoesNothing() =
        runTest {
            val page = Page(this, null)

            page.holder.reload()

            assertEquals(GlasshouseState(noStartAddress = true), page.state)
            assertEquals(emptyList<GlasshouseCommand>(), page.sent())
        }

    @Test
    fun everyKindOfProblemShowsAndStopsTheLoading() =
        runTest {
            for (problem in GlasshouseProblem.entries) {
                val page = Page(this)
                page.sent()
                page.holder.pageStarted(START)

                page.holder.pageFailed(START, problem)

                assertEquals(problem, page.state.problem)
                assertFalse(page.state.loading)
                assertEquals(START, page.state.address)
            }
        }

    @Test
    fun reloadClearsTheProblemAndOpensTheAddressThatFailed() =
        runTest {
            val page = Page(this).loaded()
            page.holder.go("offline.example")
            page.holder.pageStarted("https://offline.example")
            page.holder.pageFailed("https://offline.example", GlasshouseProblem.Offline)
            // The web view ends the load of a page that failed too; the problem stays.
            page.holder.pageFinished()
            assertEquals(GlasshouseProblem.Offline, page.state.problem)
            page.sent()

            page.holder.reload()

            assertNull(page.state.problem)
            assertTrue(page.state.loading)
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open("https://offline.example")), page.sent())
        }

    @Test
    fun aCrashedPageIsReloadedFromItsAddress() =
        runTest {
            val page = Page(this).loaded()

            page.holder.pageFailed(START, GlasshouseProblem.Crashed)
            assertEquals(GlasshouseProblem.Crashed, page.state.problem)
            assertFalse(page.state.loading)
            page.holder.reload()

            assertNull(page.state.problem)
            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open(START)), page.sent())
        }

    @Test
    fun aProblemThatFailsAgainShowsAgain() =
        runTest {
            val page = Page(this).loaded()
            page.holder.pageFailed(START, GlasshouseProblem.NotFound)

            page.holder.reload()
            page.holder.pageStarted(START)
            page.holder.pageFailed(START, GlasshouseProblem.Refused)

            assertEquals(GlasshouseProblem.Refused, page.state.problem)
            assertFalse(page.state.loading)
        }

    @Test
    fun backForwardANewAddressAndANewLoadAllClearTheProblem() =
        runTest {
            val actions =
                mapOf<String, (GlasshouseHolder) -> Unit>(
                    "back" to { it.back() },
                    "forward" to { it.forward() },
                    "go" to { it.go("example.com") },
                    "a new load" to { it.pageStarted("https://example.com") },
                )
            for ((name, action) in actions) {
                val page = Page(this).loaded(canGoBack = true, canGoForward = true)
                page.holder.pageFailed(START, GlasshouseProblem.Certificate)

                action(page.holder)

                assertNull(page.state.problem, name)
            }
        }

    @Test
    fun aNewWebViewStartsFromTheCurrentAddressAndNotFromTheOldOnesCommands() =
        runTest {
            val page = Page(this).loaded()
            page.holder.go("example.com")
            page.holder.navigated("https://example.com", canGoBack = true, canGoForward = true)
            page.holder.pageFailed("https://example.com", GlasshouseProblem.Offline)
            page.sent()
            page.stopWebView()
            // The old web view is gone, and what the person did meanwhile waits in the queue.
            page.holder.reload()
            page.holder.back()

            page.startWebView()

            assertEquals(listOf<GlasshouseCommand>(GlasshouseCommand.Open("https://example.com")), page.sent())
            assertEquals(GlasshouseState(address = "https://example.com", loading = true), page.state)
        }

    @Test
    fun aWebViewThatCollectsAgainKeepsTheHistoryAndTheQueue() =
        runTest {
            val page = Page(this).loaded(canGoBack = true)
            page.stopWebView()
            page.holder.reload()
            page.holder.back()

            page.collectAgain()

            assertEquals(
                listOf(GlasshouseCommand.Reload, GlasshouseCommand.Back),
                page.sent(),
            )
            assertEquals(GlasshouseState(address = START, loading = true, canGoBack = true), page.state)
        }
}
