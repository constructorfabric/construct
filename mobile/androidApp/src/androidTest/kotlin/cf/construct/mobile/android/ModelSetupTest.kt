package cf.construct.mobile.android

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.core.chat.Reply
import cf.construct.mobile.core.chat.Request
import cf.construct.mobile.core.chat.readRequest
import cf.construct.mobile.core.chat.responseBytes
import org.junit.Assume.assumeTrue
import org.junit.Rule
import java.net.InetAddress
import java.net.ServerSocket
import java.util.Collections
import kotlin.concurrent.thread
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val FAKE_KEY = "sk-fake-ui-0123456789-do-not-use"
private const val MODEL = "demo-model-1"
private const val COMPLETION = """{"choices":[{"message":{"content":"pong"}}]}"""
private const val TOOL_CALL =
    """{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function",""" +
        """"function":{"name":"add","arguments":"{\"a\":2,\"b\":3}"}}]}}]}"""
private const val SUM = """{"choices":[{"message":{"content":"The sum is 5."}}]}"""
private const val WAIT_MS = 20_000L
private const val NO_MODEL = "Answers: the provider has no model with this name."
private const val KEY_SAVED = "A key is saved. Leave this empty to keep it."

/** A provider on a local port that answers the three calls of a Test, and keeps what it was sent. */
private class LocalProvider(
    private val replies: List<Reply.Status>,
) {
    private val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    val requests: MutableList<Request> = Collections.synchronizedList(mutableListOf())
    val address = "http://127.0.0.1:${server.localPort}/v1"

    init {
        thread(isDaemon = true) {
            for (reply in replies) {
                val socket = runCatching { server.accept() }.getOrNull() ?: return@thread
                val input = socket.getInputStream()
                requests += readRequest { input.read() }
                socket.getOutputStream().apply {
                    write(responseBytes(reply))
                    flush()
                }
                socket.close()
            }
        }
    }

    fun close() = server.close()
}

/** Drives the real Settings page on a device: fills the setup, tests it against a fake server, removes the key. */
class ModelSetupTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private val app get() =
        InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as ConstructApp
    private var provider: LocalProvider? = null
    private var scenario: ActivityScenario<MainActivity>? = null

    @BeforeTest
    fun startFromAnEmptyPhone() {
        app.modelSettings.apply {
            removeKey()
            saveProvider("", "", null)
        }
    }

    @AfterTest
    fun cleanUp() {
        scenario?.close()
        provider?.close()
        if (keepKey) return
        app.modelSettings.apply {
            removeKey()
            saveProvider("", "", null)
        }
    }

    private fun openSettings() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        rule.onNodeWithText("Settings").performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Model").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun fill(address: String) {
        rule.onNodeWithTag("settings-address").performTextInput(address)
        rule.onNodeWithTag("settings-model").performTextInput(MODEL)
        rule.onNodeWithTag("settings-key").performTextInput(FAKE_KEY)
    }

    @Test
    fun anEmptyPhoneAsksForAnAddressAndModelAndTestIsOff() {
        openSettings()

        rule.onNodeWithTag("settings-save").performClick()

        rule.onAllNodesWithText("This field is required").assertCountEquals(2)
        rule.onAllNodesWithText("Remove key").assertCountEquals(0)
    }

    @Test
    fun setsAFakeSettingTestsItAgainstAFakeServerAndRemovesTheKey() {
        val fake =
            LocalProvider(listOf(Reply.Status(200, COMPLETION), Reply.Status(200, TOOL_CALL), Reply.Status(200, SUM)))
        provider = fake
        openSettings()
        fill(fake.address)

        rule.onNodeWithTag("settings-save").performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Saved.").fetchSemanticsNodes().isNotEmpty() }
        // The page shows that a key is set and never the key.
        rule.onNodeWithText(KEY_SAVED).assertExists()
        rule.onAllNodesWithText(FAKE_KEY, substring = true).assertCountEquals(0)
        assertEquals(FAKE_KEY, app.modelSettings.readProvider().key)

        rule.onNodeWithTag("settings-test").assertIsEnabled().performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Uses tools: works").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Answers: works").assertExists()
        assertEquals(3, fake.requests.size)
        assertTrue(fake.requests.all { it.authorization == "Bearer $FAKE_KEY" })
        assertTrue(fake.requests.all { it.body.contains("\"$MODEL\"") })

        rule.onNodeWithTag("settings-remove-key").performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Remove key").fetchSemanticsNodes().isEmpty() }
        assertNull(app.modelSettings.readProvider().key)
        assertFalse(app.modelSettings.read().keyIsSet)
        assertEquals(fake.address, app.modelSettings.read().address)
    }

    @Test
    fun aModelTheProviderDoesNotKnowShowsTheReasonInPlainWords() {
        val fake = LocalProvider(listOf(Reply.Status(404, """{"error":{"message":"no such model"}}""")))
        provider = fake
        openSettings()
        fill(fake.address)

        rule.onNodeWithTag("settings-test").performClick()
        rule.waitUntil(WAIT_MS) {
            rule.onAllNodesWithText(NO_MODEL).fetchSemanticsNodes().isNotEmpty()
        }

        rule.onNodeWithText("Uses tools: skipped").assertExists()
    }

    @Test
    fun theSettingStaysAfterTheScreenIsStartedAgain() {
        openSettings()
        fill("http://127.0.0.1:9/v1")
        rule.onNodeWithTag("settings-save").performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Saved.").fetchSemanticsNodes().isNotEmpty() }
        scenario?.close()

        openSettings()

        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText(KEY_SAVED).fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("http://127.0.0.1:9/v1").assertCountEquals(1)
    }

    /** Run by `scripts/check-key-not-leaked.sh`: saves a recognizable key and leaves it for the file and log search. */
    @Test
    fun leavesAFakeKeyForTheLeakSearch() {
        val enabled = InstrumentationRegistry.getArguments().getString("leakSearch") == "1"
        assumeTrue("only the leak search script sets this", enabled)
        openSettings()
        rule.onNodeWithTag("settings-address").performTextInput("http://127.0.0.1:9/v1")
        rule.onNodeWithTag("settings-model").performTextInput(MODEL)
        rule.onNodeWithTag("settings-key").performTextInput("sk-android-leak-search-0123456789-do-not-use")
        rule.onNodeWithTag("settings-save").performClick()
        rule.waitUntil(WAIT_MS) { rule.onAllNodesWithText("Saved.").fetchSemanticsNodes().isNotEmpty() }
        scenario?.close()
        // Leave the key saved: the script searches the app's files next, then clears the app's data.
        keepKey = true
    }

    private var keepKey = false
}
