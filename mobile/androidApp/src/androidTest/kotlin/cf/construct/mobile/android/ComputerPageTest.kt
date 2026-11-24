package cf.construct.mobile.android

import android.graphics.Bitmap
import android.os.SystemClock
import android.util.Log
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.core.database.seedWithSampleData
import cf.construct.mobile.core.keys.androidPairingSecret
import cf.construct.mobile.feature.status.ConnectionStatusKind
import org.junit.Assume.assumeTrue
import org.junit.Rule
import java.io.File
import java.security.MessageDigest
import kotlin.test.Test
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Run against the current agent example with scripts/test-phone-pairing.py. The app checks the computer every
 * [SLOW_POLL_MILLIS] here, much longer than the test waits, so the Try again button is the only way the state
 * can come back after the computer returns.
 */
class ComputerPageTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val directory = File(context.filesDir, "computer-pairing-test")
    private val store = androidPairingSecret(context)

    @Test
    fun computerPage() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("computerPairingFixture") == "enabled")
        // The canary: the script must find this line in the device log, or its check for pairing material
        // is not looking at the log. It is the SHA-256 path of that check; the unit test of the script
        // covers the plain-text path.
        Log.i("PhonePairingFixture", "c3".repeat(32))
        store.clear()
        PhonePolling.intervalMillis = SLOW_POLL_MILLIS
        val app = context.applicationContext as ConstructApp
        try {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                openSettings()
                openComputerRow()
                pairFromSettings()
                app.copy.seedWithSampleData()
                assertTrue(app.copy.conversations().isNotEmpty())
                assertTrue(
                    app.copy
                        .facts()
                        .facts
                        .isNotEmpty(),
                )
                backToSettings()
                capture("settings-connected")
                openComputerRow()
                verifyOutage(scenario)

                compose.onNodeWithText("Unpair").performClick()
                capture("confirm-unpair")
                compose.onNodeWithTag("confirm-unpair").performClick()
                waitForText("Not paired")
                assertNull(store.read())
                assertTrue(app.copy.conversations().isEmpty())
                assertTrue(
                    app.copy
                        .facts()
                        .facts
                        .isEmpty(),
                )
                backToSettings()
            }
        } finally {
            store.clear()
        }
    }

    private fun pairFromSettings() {
        val offer = File(directory, "offer").readLines()
        check(offer.size == 2) { "The pairing fixture is incomplete." }
        waitForText("Not paired")
        capture("not-paired")
        compose.onNodeWithText("Enter address and code").assertIsEnabled().performClick()
        waitForText("Pair with a computer")
        capture("manual")
        compose.onNodeWithTag("computer-address").performTextInput(offer[0])
        compose.onNodeWithTag("computer-code").performTextInput(offer[1])
        compose.onNodeWithText("Pair").performScrollTo().performClick()
        waitForText("Paired computer")
        waitForText("Connected")
        recordDigest()
        capture("connected")
    }

    private fun verifyOutage(scenario: ActivityScenario<MainActivity>) {
        signal("computer-stop")
        waitForFile("computer-stopped")
        showOutage(scenario)
        waitUntilDisplayed(checkNotNull(ConnectionStatusKind.Unreachable.detail))
        waitUntilDisplayed("Last contact:")
        compose.onAllNodesWithText("Try again").assertCountEquals(1)
        capture("unreachable")
        compose.onNodeWithText("Pair again").performClick()
        compose.onNodeWithText("Scan QR code").assertIsDisplayed().assertIsEnabled()
        compose.onNodeWithText("Pair again").performClick()

        // The connection line shows the same state on the other pages.
        backToSettings()
        capture("settings-unreachable")
        compose.onNodeWithText("Chat").performClick()
        waitForText(ConnectionStatusKind.Unreachable.title)
        capture("chat-unreachable")
        compose.onNodeWithText("Settings").performClick()
        waitForText("Computer")
        openComputerRow()

        signal("computer-retry")
        waitForFile("computer-restarted")
        retryUntilConnected()
    }

    /**
     * The checks come every [SLOW_POLL_MILLIS], so the outage is found when the app comes back to the front
     * and checks at once. The first check can still reach the computer while it is shutting down, so go again.
     */
    private fun showOutage(scenario: ActivityScenario<MainActivity>) {
        repeat(OUTAGE_TRIES) {
            scenario.moveToState(Lifecycle.State.CREATED)
            scenario.moveToState(Lifecycle.State.RESUMED)
            if (shown("Not reachable", ATTEMPT_MILLIS)) return
        }
        error("The app did not show that the computer is not reachable.")
    }

    /** Only a click on Try again can bring the state back in time: the next check is minutes away. */
    private fun retryUntilConnected() {
        repeat(RETRY_TRIES) {
            if (compose.onAllNodesWithText("Try again").fetchSemanticsNodes().isNotEmpty()) {
                compose.onNodeWithText("Try again").performClick()
            }
            if (shown("Connected", ATTEMPT_MILLIS)) return
        }
        error("Try again did not bring the connection back.")
    }

    private fun openSettings() {
        compose.onNodeWithText("Settings").performClick()
        waitForText("Computer")
    }

    private fun openComputerRow() {
        compose.onNodeWithTag("open-computer").performScrollTo().performClick()
    }

    private fun backToSettings() {
        compose.onNodeWithContentDescription("Back").performClick()
        waitForText("Model")
    }

    private fun waitForText(text: String) {
        compose.waitUntil(60_000) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    /** The page is laid out again when the app comes back to the front, so wait until the text is on screen. */
    private fun waitUntilDisplayed(text: String) {
        compose.waitUntil(60_000) {
            runCatching { compose.onNodeWithText(text, substring = true).assertIsDisplayed() }.isSuccess
        }
    }

    private fun shown(
        text: String,
        timeoutMillis: Long,
    ) = try {
        compose.waitUntil(timeoutMillis) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        true
    } catch (_: ComposeTimeoutException) {
        false
    }

    private fun signal(name: String) = File(directory, name).writeText("ready")

    private fun waitForFile(name: String) {
        val end = SystemClock.uptimeMillis() + HOST_MILLIS
        while (!File(directory, name).exists()) {
            check(SystemClock.uptimeMillis() < end) { "The host did not signal $name." }
            Thread.sleep(200)
        }
    }

    private fun recordDigest() {
        val token =
            checkNotNull(
                store
                    .read()
                    ?.lineSequence()
                    ?.drop(2)
                    ?.firstOrNull(),
            ) {
                "The pairing credential was not saved."
            }
        val digest = MessageDigest.getInstance("SHA-256").digest(token.encodeToByteArray()).toHexString()
        File(directory, "computer-digest").writeText(digest)
    }

    private fun capture(name: String) {
        if (InstrumentationRegistry.getArguments().getString("computerScreenshots") != "enabled") return
        compose.waitForIdle()
        val screenshot = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
        try {
            File(directory, "$name.png").outputStream().use {
                check(screenshot.compress(Bitmap.CompressFormat.PNG, 100, it))
            }
        } finally {
            screenshot.recycle()
        }
    }

    private companion object {
        const val SLOW_POLL_MILLIS = 120_000L
        const val ATTEMPT_MILLIS = 5_000L
        const val OUTAGE_TRIES = 6
        const val RETRY_TRIES = 6
        const val HOST_MILLIS = 60_000L
    }
}
