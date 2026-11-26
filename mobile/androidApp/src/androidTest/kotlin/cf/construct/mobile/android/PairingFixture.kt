package cf.construct.mobile.android

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.os.SystemClock
import android.util.Log
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.core.keys.PairingSecret
import cf.construct.mobile.core.keys.androidPairingSecret
import java.io.File
import java.security.MessageDigest

/**
 * What the tests of the Computer page and of the sync share: the folder in the app's files that the script
 * (scripts/test-phone-pairing.py) reads and writes through adb, the pairing offer it leaves there, the credential
 * digest and the screenshots the test leaves for it, and the waits of the app's screen.
 *
 * The test asks the script to act on the computer with [signal] and [ask]; the script answers with a file.
 */
internal class PairingFixture(
    private val compose: ComposeTestRule,
) {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    val directory = File(instrumentation.targetContext.filesDir, "computer-pairing-test")
    val store: PairingSecret = androidPairingSecret(instrumentation.targetContext)
    val app = instrumentation.targetContext.applicationContext as ConstructApp

    /**
     * What a run starts from: no pairing, a copy that holds nothing (the debug build's sample data is not waiting for
     * its first copy), and checks so far apart that only the test makes the app talk to the computer.
     */
    fun start() {
        // The script looks for this line in the device log, or its check for pairing material is not reading the
        // log. It is the SHA-256 path of that check; the unit test of the script covers the plain-text path.
        Log.i("PhonePairingFixture", "c3".repeat(32))
        store.clear()
        PhonePolling.intervalMillis = SLOW_POLL_MILLIS
        app.copy.deleteEverything()
    }

    fun openSettings() {
        compose.onNodeWithText("Settings").performClick()
        waitForText("Computer")
    }

    fun openComputerRow() {
        compose.onNodeWithTag("open-computer").performScrollTo().performClick()
    }

    fun backToSettings() {
        compose.onNodeWithContentDescription("Back").performClick()
        waitForText("Model")
    }

    /** Presses the system's back, the way the back button or gesture does, and waits for the app to deal with it. */
    fun pressBack() {
        instrumentation.uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)
        instrumentation.waitForIdleSync()
    }

    /** Pairs with the computer's offer on the Computer page of an app that is not paired, by address and code. */
    fun pairByCode() {
        val offer = offer()
        waitForText("Not paired")
        capture("not-paired")
        compose.onNodeWithText("Enter address and code").assertIsEnabled().performClick()
        waitForText("Pair with a computer")
        capture("manual")
        compose.onNodeWithTag("computer-address").performTextInput(offer[0])
        compose.onNodeWithTag("computer-code").performTextInput(offer[1])
        compose.onNodeWithText("Pair").performScrollTo().performClick()
    }

    /** The address and the code of the computer's pairing offer. */
    fun offer(): List<String> {
        val offer = File(directory, "offer").readLines()
        check(offer.size == 2) { "The pairing fixture is incomplete." }
        return offer
    }

    fun signal(name: String) = File(directory, name).writeText("ready")

    fun waitForFile(name: String) {
        val end = SystemClock.uptimeMillis() + HOST_MILLIS
        while (!File(directory, name).exists()) {
            check(SystemClock.uptimeMillis() < end) { "The host did not signal $name." }
            Thread.sleep(POLL_MILLIS)
        }
    }

    /** Asks the script to do [command] to the computer, and returns when it has. */
    fun ask(command: String) {
        val reply = File(directory, "sync-reply")
        reply.delete()
        // The script reads the request as soon as it exists, so it is written whole under another name first.
        val part = File(directory, "sync-request.part")
        part.writeText(command)
        check(part.renameTo(File(directory, "sync-request"))) { "The request $command could not be left." }
        waitForFile("sync-reply")
        check(reply.readText() == "ready") { "The host did not do $command." }
    }

    /** Leaves the SHA-256 of the phone's credential for the script, which compares it with the computer's. */
    fun recordDigest(
        store: PairingSecret,
        name: String,
    ) {
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
        File(directory, name).writeText(digest)
    }

    /**
     * Leaves a screenshot of a state that does not last. It fails when [text] was gone by the time the picture was
     * taken, so a picture of the wrong state is never left for the script.
     */
    fun captureWhileShown(
        name: String,
        text: String,
    ) {
        if (!screenshotsAsked()) return
        capture(name)
        check(shownContaining(text)) { "\"$text\" was gone before the screenshot $name was taken." }
    }

    /** Leaves a screenshot of the screen for the script, when the run is asked for them. */
    fun capture(name: String) {
        if (!screenshotsAsked()) return
        compose.waitForIdle()
        // The test's clock holds a spinner at its first frame, where it is a dot. Moving it on shows it as an arc.
        compose.mainClock.advanceTimeBy(SPINNER_MILLIS)
        compose.waitForIdle()
        // The screen is drawn a frame after the page is composed, and the screenshot is of what is drawn.
        Thread.sleep(DRAW_MILLIS)
        val screenshot = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
        try {
            File(directory, "$name.png").outputStream().use {
                check(screenshot.compress(Bitmap.CompressFormat.PNG, 100, it))
            }
        } finally {
            screenshot.recycle()
        }
    }

    /** Waits until a node has exactly this text. */
    fun waitForText(text: String) = waitUntil { shownExactly(text) }

    /** Waits until a node has this text in it. */
    fun waitForTextContaining(text: String) = waitUntil { shownContaining(text) }

    /** The page is laid out again when the app comes back to the front, so wait until the text is on screen. */
    fun waitUntilDisplayed(text: String) =
        waitUntil { runCatching { compose.onNodeWithText(text, substring = true).assertIsDisplayed() }.isSuccess }

    /** Waits until no node has this text in it. */
    fun waitUntilGone(
        text: String,
        timeoutMillis: Long = WAIT_MILLIS,
    ) = waitUntil(timeoutMillis) { !shownContaining(text) }

    /** Whether a node shows this text within [timeoutMillis]. */
    fun appears(
        text: String,
        timeoutMillis: Long,
    ) = try {
        compose.waitUntil(timeoutMillis) { shownContaining(text) }
        true
    } catch (_: ComposeTimeoutException) {
        false
    }

    fun shownContaining(text: String) =
        compose.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().isNotEmpty()

    private fun screenshotsAsked() =
        InstrumentationRegistry.getArguments().getString("computerScreenshots") == "enabled"

    private fun shownExactly(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun waitUntil(
        timeoutMillis: Long = WAIT_MILLIS,
        condition: () -> Boolean,
    ) = compose.waitUntil(timeoutMillis, condition)

    private companion object {
        const val SLOW_POLL_MILLIS = 120_000L
        const val WAIT_MILLIS = 60_000L
        const val HOST_MILLIS = 60_000L
        const val POLL_MILLIS = 200L
        const val SPINNER_MILLIS = 500L
        const val DRAW_MILLIS = 400L
    }
}
