package cf.construct.mobile.android

import android.content.Intent
import android.util.Log
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.LinkSnapshot
import cf.construct.mobile.core.connection.PairResult
import cf.construct.mobile.core.connection.defaultPhoneConnection
import cf.construct.mobile.core.keys.androidPairingSecret
import cf.construct.mobile.feature.status.ConnectionStatusKind
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assume.assumeTrue
import org.junit.Rule
import java.io.File
import java.security.MessageDigest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/** Run on a disposable emulator using scripts/test-phone-pairing.py and the agent example. */
class PhonePairingTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val directory = File(context.filesDir, "computer-pairing-test")
    private val store = androidPairingSecret(context)
    private val state = MutableStateFlow(LinkSnapshot(ComputerLink.NotPaired, 0))

    @Test
    fun pairWithPreviousAgent(): Unit =
        runBlocking {
            assumeFixture()
            store.clear()
            val offer = readOffer("previous-offer")
            val connection = createPhoneConnection(context)
            assertEquals(PairResult.ComputerUpdate, connection.pairByCode(offer.address, offer.code, NAME))
            assertTrue(store.read() == null)
            assertEquals(PairResult.Paired, connection.pairByQr(File(directory, "previous-qr").readText(), NAME))
            recordCredentialDigest("previous")
            val watch = launch { connection.monitor { state.value = it } }
            try {
                await(ComputerLink.Connected)
            } finally {
                watch.cancelAndJoin()
                store.clear()
            }
        }

    @Test
    fun pairAndUnpair(): Unit =
        runBlocking {
            assumeFixture()
            Log.i("PhonePairingFixture", "c3".repeat(32))
            store.clear()
            val offer = readOffer("offer")
            val old = defaultPhoneConnection(store, { true }, protocolVersion = 0)
            assertEquals(PairResult.PhoneUpdate, old.pairByCode(offer.address, offer.code, NAME))
            val connection = createPhoneConnection(context)
            val wrongQr = File(directory, "wrong-qr").readText()
            assertEquals(PairResult.WrongComputer, connection.pairByQr(wrongQr, NAME))
            assertTrue(store.read() == null, "A mismatched QR certificate was accepted.")
            assertEquals(PairResult.Paired, connection.pairByCode(offer.address, offer.code, NAME))
            recordCredentialDigest("first")
            assertEquals(PairResult.FreshCode, connection.pairByCode(offer.address, offer.code, NAME))
            val watch = launch { connection.monitor { state.value = it } }
            try {
                await(ComputerLink.Connected)
                signal("revoke")
                await(ComputerLink.NotPaired)
                assertTrue(store.read() == null, "The revoked pairing is still stored.")

                signal("next")
                val qr = readFixture("second-qr")
                assertEquals(PairResult.Paired, connection.pairByQr(qr, NAME))
                recordCredentialDigest("second")
                await(ComputerLink.Connected)
            } finally {
                watch.cancelAndJoin()
            }
        }

    @Test
    fun reconnectAfterAppRestart() {
        assumeFixture()
        val intent = Intent(context, MainActivity::class.java).putExtra("pairing-frame-test", true)
        try {
            ActivityScenario.launch<MainActivity>(intent).use {
                val down = ConnectionStatusKind.Unreachable
                waitForLine(down.title)
                compose.onNodeWithText(checkNotNull(down.detail)).assertIsDisplayed()
                compose.onNodeWithText("Last contact:", substring = true).assertIsDisplayed()
                runBlocking { readFixture("online") }
                compose.onNodeWithText("Try again").performClick()
                waitForLine(ConnectionStatusKind.Connected.title)
            }
        } finally {
            store.clear()
        }
    }

    private fun waitForLine(title: String) {
        compose.waitUntil(25_000) { compose.onAllNodesWithText(title).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun assumeFixture() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("computerPairingFixture") == "enabled")
    }

    private fun signal(name: String) = File(directory, name).writeText("ready")

    private fun recordCredentialDigest(label: String) {
        val tokenLine =
            store
                .read()
                ?.lineSequence()
                ?.drop(2)
                ?.firstOrNull()
        val token = checkNotNull(tokenLine) { "The credential was not saved." }
        val digest = MessageDigest.getInstance("SHA-256").digest(token.encodeToByteArray()).toHexString()
        File(directory, "$label-digest").writeText(digest)
    }

    private suspend fun readFixture(name: String): String =
        withTimeout(60.seconds) {
            val file = File(directory, name)
            while (!file.isFile) delay(100.milliseconds)
            file.readText()
        }

    private suspend fun readOffer(name: String): Offer {
        val fields = readFixture(name).trimEnd().lines()
        check(fields.size == 2) { "The pairing fixture is incomplete." }
        return Offer(fields[0], fields[1])
    }

    private suspend fun await(link: ComputerLink): LinkSnapshot =
        withTimeout(25.seconds) {
            state.first { it.link == link }
        }

    private class Offer(
        val address: String,
        val code: String,
    )

    private companion object {
        const val NAME = "Android test phone"
    }
}
