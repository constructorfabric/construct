package cf.construct.mobile.android

import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeTrue
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Run against the current agent example with scripts/test-phone-pairing.py. The example serves a chat and two facts,
 * so the first copy after the pairing has something to copy. The app checks the computer much less often here than
 * the test waits (see [PairingFixture.start]), so the Try again button is the only way the state can come back
 * after the computer returns.
 */
class ComputerPageTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private val fixture = PairingFixture(compose)
    private val store = fixture.store
    private val app = fixture.app

    @Test
    fun computerPage() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("computerPairingFixture") == "enabled")
        fixture.start()
        try {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                fixture.openSettings()
                fixture.openComputerRow()
                pairFromSettings()
                // What the first copy brought is in the copy of the phone.
                compose.waitUntil(WAIT_MILLIS) { app.copy.conversations().isNotEmpty() }
                assertTrue(
                    app.copy
                        .facts()
                        .facts
                        .isNotEmpty(),
                )
                fixture.backToSettings()
                fixture.capture("settings-connected")
                fixture.openComputerRow()
                verifyOutage(scenario)

                compose.onNodeWithText("Unpair").performClick()
                fixture.capture("confirm-unpair")
                compose.onNodeWithTag("confirm-unpair").performClick()
                fixture.waitForText("Not paired")
                assertNull(store.read())
                assertTrue(app.copy.conversations().isEmpty())
                assertTrue(
                    app.copy
                        .facts()
                        .facts
                        .isEmpty(),
                )
                fixture.backToSettings()
            }
        } finally {
            store.clear()
        }
    }

    private fun pairFromSettings() {
        fixture.pairByCode()
        // The first copy follows the pairing, and the page comes back when it is whole.
        fixture.waitForText("Paired computer")
        fixture.waitForText("Connected")
        fixture.recordDigest(store, "computer-digest")
        fixture.capture("connected")
    }

    private fun verifyOutage(scenario: ActivityScenario<MainActivity>) {
        fixture.signal("computer-stop")
        fixture.waitForFile("computer-stopped")
        showOutage(scenario)
        fixture.waitUntilDisplayed("The computer is not reachable")
        fixture.waitUntilDisplayed("Last sync")
        compose.onAllNodesWithText("Try again").assertCountEquals(1)
        fixture.capture("unreachable")
        compose.onNodeWithText("Pair again").performClick()
        compose.onNodeWithText("Scan QR code").assertIsDisplayed().assertIsEnabled()
        compose.onNodeWithText("Pair again").performClick()

        // The pages with something to sync tell the same: the notice at the top of the page.
        fixture.backToSettings()
        fixture.capture("settings-unreachable")
        compose.onNodeWithText("Chat").performClick()
        fixture.waitForTextContaining("The computer is not reachable. Last sync")
        fixture.capture("chat-unreachable")
        compose.onNodeWithText("Settings").performClick()
        fixture.waitForText("Computer")
        fixture.openComputerRow()

        fixture.signal("computer-retry")
        fixture.waitForFile("computer-restarted")
        retryUntilConnected()
    }

    /**
     * The checks are minutes apart, so the outage is found when the app comes back to the front
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

    private fun shown(
        text: String,
        timeoutMillis: Long,
    ) = try {
        compose.waitUntil(timeoutMillis) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        true
    } catch (_: ComposeTimeoutException) {
        false
    }

    private companion object {
        const val ATTEMPT_MILLIS = 5_000L
        const val WAIT_MILLIS = 60_000L
        const val OUTAGE_TRIES = 6
        const val RETRY_TRIES = 6
    }
}
