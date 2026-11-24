package cf.construct.mobile.android

import android.graphics.Bitmap
import android.util.Log
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

/** Run against the current agent example with scripts/test-phone-pairing.py android --computer-page. */
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
        Log.i("PhonePairingFixture", "c3".repeat(32))
        store.clear()
        val app = context.applicationContext as ConstructApp
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                openComputer()
                pairFromSettings()
                app.copy.seedWithSampleData()
                assertTrue(app.copy.conversations().isNotEmpty())
                assertTrue(
                    app.copy
                        .facts()
                        .facts
                        .isNotEmpty(),
                )
                verifyOutage()

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
                compose.onNodeWithContentDescription("Back").performClick()
                waitForText("Model")
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

    private fun verifyOutage() {
        signal("computer-stop")
        waitForText("Not reachable")
        val detail = checkNotNull(ConnectionStatusKind.Unreachable.detail)
        compose.onNodeWithText(detail, substring = true).assertIsDisplayed()
        compose.onNodeWithText("Last contact:", substring = true).assertIsDisplayed()
        compose.onAllNodesWithText("Try again").assertCountEquals(1)
        capture("unreachable")
        compose.onNodeWithText("Pair again").performClick()
        compose.onNodeWithText("Scan QR code").assertIsDisplayed().assertIsEnabled()
        compose.onNodeWithText("Pair again").performClick()
        compose.onNodeWithText("Try again").performClick()
        signal("computer-retry")
        waitForText("Connected")
    }

    private fun openComputer() {
        compose.onNodeWithText("Settings").performClick()
        waitForText("Computer")
        compose.onNodeWithTag("open-computer").performScrollTo().performClick()
    }

    private fun waitForText(text: String) {
        compose.waitUntil(60_000) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun signal(name: String) = File(directory, name).writeText("ready")

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
}
