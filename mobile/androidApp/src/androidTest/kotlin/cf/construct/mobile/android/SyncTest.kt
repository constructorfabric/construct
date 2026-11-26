package cf.construct.mobile.android

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.feature.status.SYNC_INTERVAL_MILLIS
import org.junit.Assume.assumeTrue
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The sync against the computer's test program, which serves made-up facts and chats from a file that the script
 * (scripts/test-phone-pairing.py --sync) changes while the test runs. The app checks every [CHECK_MILLIS] and waits
 * [PAUSE_MILLIS] before each step, so the test can see the loading screen, its counts and "Syncing". The test asks
 * the script for a new chat, and to stop and start the computer.
 */
class SyncTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private val fixture = PairingFixture(compose)
    private val store = fixture.store

    @Test
    fun sync() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("computerPairingFixture") == "enabled")
        fixture.start()
        SyncTesting.pauseMillis = PAUSE_MILLIS
        try {
            launch(CHECK_MILLIS) {
                fixture.openSettings()
                fixture.openComputerRow()
                fixture.pairByCode()
                verifyFirstCopy()
                verifyPagesHoldTheComputersData()
            }
            // Long pauses, so that "Syncing" stays up while the screenshot of it is taken.
            SyncTesting.pauseMillis = OPEN_PAUSE_MILLIS
            launch(CHECK_MILLIS) {
                verifyTheSyncWhenTheAppOpens()
                verifyChecksAndNewChat()
                verifyNotReachable()
            }
            launch(FEW_CHECKS_MILLIS) { verifyTryAgainAfterStart() }
            launch(FEW_CHECKS_MILLIS) { verifyComputerPage() }
        } finally {
            SyncTesting.pauseMillis = 0
            SyncTesting.intervalMillis = SYNC_INTERVAL_MILLIS
            store.clear()
        }
    }

    /** Starts the app again, as a person does: its sync starts with it, and its checks come every [checkMillis]. */
    private fun launch(
        checkMillis: Long,
        steps: () -> Unit,
    ) {
        SyncTesting.intervalMillis = checkMillis
        ActivityScenario.launch(MainActivity::class.java).use { steps() }
    }

    /** A row of the loading screen as it is read out: its name and its count. */
    private fun row(name: String): String =
        compose
            .onNodeWithText(name)
            .fetchSemanticsNode()
            .config[SemanticsProperties.Text]
            .joinToString(", ") { it.text }

    private fun verifyFirstCopy() {
        fixture.waitForText(LOADING)
        // The tabs are under the loading screen: they are out of reach of a touch and of a screen reader.
        compose.onAllNodesWithText("Settings").assertCountEquals(0)
        // Back does not reach the page under it: it is the Computer page, one step below Settings.
        fixture.pressBack()
        assertTrue(fixture.shownContaining(LOADING), "Back closed the loading screen")
        // The counts rise: the facts are copied first, then the chats one by one, then the dashboard.
        fixture.waitForText("Step 2 of 3")
        assertContains(row("Memory"), "12 facts")
        // The chats are under way, and the dashboard waits. A row under way says "copying", then the count of the
        // list as it is read ("4 chats"), then "k of 4": the test does not look for one of them.
        assertFalse(row("Chats").contains("waiting"), "Chats says ${row("Chats")}")
        assertContains(row("Dashboard"), "waiting")
        fixture.capture("loading")
        fixture.waitForText("Step 3 of 3")
        assertContains(row("Chats"), "4 chats")
        fixture.waitUntilGone(LOADING)
        // The first copy is not followed by "Synced just now", and it leaves the page the person was on.
        fixture.waitForText("Paired computer")
        fixture.recordDigest(store, "computer-digest")
        assertFalse(fixture.shownContaining(JUST_SYNCED))
    }

    private fun verifyPagesHoldTheComputersData() {
        fixture.backToSettings()
        compose.onNodeWithText("Memory").performClick()
        fixture.waitUntilDisplayed("Skill 1")
        compose.onNodeWithText("Chat").performClick()
        fixture.waitUntilDisplayed("Plan the trip")
        assertTrue(fixture.shownContaining("Garden ideas"))
    }

    /** A sync when the app opens shows "Syncing", then "Synced just now" for a few seconds, then nothing. */
    private fun verifyTheSyncWhenTheAppOpens() {
        fixture.waitForText(SYNCING)
        fixture.captureWhileShown("syncing", SYNCING)
        // The syncs that follow are paced as before; the one that is under way keeps its long pauses.
        SyncTesting.pauseMillis = PAUSE_MILLIS
        fixture.waitForText(JUST_SYNCED)
        fixture.captureWhileShown("justsynced", JUST_SYNCED)
        fixture.waitUntilGone(JUST_SYNCED, TIMER_WAIT_MILLIS)
    }

    /** The checks every few seconds show nothing, and a chat the program gets appears in the open list. */
    private fun verifyChecksAndNewChat() {
        // Three checks pass (the app asks every 6 seconds in this run) and none of them says anything.
        assertFalse(fixture.appears(SYNCING, QUIET_MILLIS), "a check showed \"Syncing\"")
        assertFalse(fixture.shownContaining(UNREACHABLE))
        assertFalse(fixture.shownContaining(NEW_CHAT))
        fixture.ask("chat")
        fixture.waitUntilDisplayed(NEW_CHAT)
        assertFalse(fixture.shownContaining(SYNCING))
    }

    /** "Not reachable" shows once with "Try again", not on the Glasshouse page, and goes when a sync works. */
    private fun verifyNotReachable() {
        fixture.ask("stop")
        fixture.waitForTextContaining(UNREACHABLE)
        assertTrue(fixture.shownContaining("$UNREACHABLE. Last sync"))
        compose.onAllNodesWithText("Try again").assertCountEquals(1)
        fixture.capture("unreachable")
        // The checks that follow do not tell it again: the notice is the same one, and "Syncing" never shows.
        assertFalse(fixture.appears(SYNCING, QUIET_MILLIS), "a check showed \"Syncing\"")
        assertTrue(fixture.shownContaining(UNREACHABLE))
        compose.onNodeWithText("Glasshouse").performClick()
        compose.waitForIdle()
        assertFalse(fixture.shownContaining(UNREACHABLE), "a notice on the Glasshouse page")
    }

    /**
     * The computer stays off while the app is started again: the sync of the start misses it and shows the notice, and
     * no check can clear it before "Try again" does.
     */
    private fun verifyTryAgainAfterStart() {
        fixture.waitForTextContaining(UNREACHABLE)
        fixture.ask("restart")
        compose.onNodeWithText("Try again").performClick()
        fixture.waitForText(JUST_SYNCED)
        assertFalse(fixture.shownContaining(UNREACHABLE))
        fixture.waitUntilGone(JUST_SYNCED, TIMER_WAIT_MILLIS)
    }

    private fun verifyComputerPage() {
        fixture.waitForText(JUST_SYNCED)
        fixture.waitUntilGone(JUST_SYNCED, TIMER_WAIT_MILLIS)
        fixture.openSettings()
        fixture.openComputerRow()
        fixture.waitForTextContaining("Last sync ")
        fixture.waitForText("Sync now")
        fixture.capture("computer")
        compose.onNodeWithText("Sync now").performClick()
        fixture.waitForText("Syncing…")
        fixture.waitForText("Sync now")
    }

    private companion object {
        const val LOADING = "Copying from your computer"
        const val SYNCING = "Syncing with your computer."
        const val JUST_SYNCED = "Up to date. Synced just now."
        const val UNREACHABLE = "The computer is not reachable"
        const val NEW_CHAT = "A chat from the computer"
        const val PAUSE_MILLIS = 900L
        const val OPEN_PAUSE_MILLIS = 2_500L
        const val CHECK_MILLIS = 6_000L
        const val FEW_CHECKS_MILLIS = 600_000L
        const val QUIET_MILLIS = 20_000L
        const val TIMER_WAIT_MILLIS = 10_000L
    }
}
