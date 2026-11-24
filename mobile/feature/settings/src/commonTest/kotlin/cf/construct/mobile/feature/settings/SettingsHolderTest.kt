package cf.construct.mobile.feature.settings

import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.chat.ModelTestResult
import cf.construct.mobile.core.keys.FakeBacking
import cf.construct.mobile.core.keys.FakeModelSettingsStore
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.core.keys.SecretValue
import cf.construct.mobile.core.keys.StoredModelSettings
import cf.construct.mobile.core.keys.TextValues
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Runnable
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.coroutines.CoroutineContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val FAKE_KEY = "sk-fake-0123456789-do-not-use"

/** A chat core that answers a Test with [result] and remembers what it was asked. */
private class RecordingCore(
    var result: ModelTestResult = ModelTestResult.Works("pong"),
) : ChatCore {
    val asked = mutableListOf<Triple<String, String, String>>()

    override fun fixedRequestBody(toolsFile: String) = error("not used")

    override fun testModel(
        baseUrl: String,
        model: String,
        apiKey: String,
    ): ModelTestResult {
        asked += Triple(baseUrl, model, apiKey)
        return result
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
class SettingsHolderTest {
    private val backing = FakeBacking()
    private val store = FakeModelSettingsStore(backing)
    private val core = RecordingCore()

    private fun TestScope.holder(store: ModelSettingsStore = this@SettingsHolderTest.store) =
        UnconfinedTestDispatcher(testScheduler).let { immediate ->
            SettingsHolder(store, core, CoroutineScope(immediate), immediate)
        }

    private fun SettingsHolder.fill() {
        change(SettingsField.Address, "http://127.0.0.1:8080/v1")
        change(SettingsField.ModelName, "demo-model")
        change(SettingsField.Key, FAKE_KEY)
    }

    @Test
    fun anEmptyPhoneShowsTheEmptyForm() =
        runTest {
            val state = holder().state.value

            assertFalse(state.loading)
            assertEquals(SettingsState(loading = false), state)
            assertFalse(state.canTest)
        }

    @Test
    fun saveKeepsTextAndKeyAndThePageShowsOnlyThatAKeyIsSet() =
        runTest {
            val holder = holder()
            holder.fill()

            holder.save()

            val state = holder.state.value
            assertTrue(state.saved)
            assertTrue(state.keyIsSet)
            assertEquals("", state.keyEntry)
            assertFalse(state.toString().contains(FAKE_KEY))
            assertEquals("demo-model", store.read().model)
            assertEquals(FAKE_KEY, store.readProvider().key)
        }

    @Test
    fun aSettingSurvivesARestart() =
        runTest {
            holder().apply { fill() }.save()

            val again = holder(FakeModelSettingsStore(backing)).state.value

            assertEquals("http://127.0.0.1:8080/v1", again.address)
            assertEquals("demo-model", again.model)
            assertTrue(again.keyIsSet)
        }

    @Test
    fun saveWithAnEmptyAddressOrModelSaysWhichFieldIsRequired() =
        runTest {
            val holder = holder()
            holder.change(SettingsField.ModelName, "demo-model")

            holder.save()

            assertTrue(holder.state.value.addressMissing)
            assertFalse(holder.state.value.modelMissing)
            assertFalse(holder.state.value.saved)
            assertEquals("", store.read().model)
        }

    @Test
    fun leavingTheKeyEmptyKeepsTheSavedKey() =
        runTest {
            val holder = holder()
            holder.fill()
            holder.save()

            holder.change(SettingsField.ModelName, "other-model")
            holder.save()

            assertEquals(FAKE_KEY, store.readProvider().key)
            assertEquals("other-model", store.read().model)
        }

    @Test
    fun removeKeyLeavesTheKeyGoneAndTheTextThere() =
        runTest {
            val holder = holder()
            holder.fill()
            holder.save()

            holder.removeKey()

            assertFalse(holder.state.value.keyIsSet)
            assertNull(store.readProvider().key)
            assertEquals("demo-model", store.read().model)
        }

    @Test
    fun testCallsTheCoreWithTheTypedKeyAndShowsBothChecksWorking() =
        runTest {
            val holder = holder()
            holder.fill()

            holder.test()

            assertEquals(listOf(Triple("http://127.0.0.1:8080/v1", "demo-model", FAKE_KEY)), core.asked)
            val check = holder.state.value.check as SettingsCheck.Shown
            assertEquals(CheckLine(CheckStatus.Works), check.answers)
            assertEquals(CheckLine(CheckStatus.Works), check.tools)
            assertFalse(holder.state.value.testing)
        }

    @Test
    fun testUsesTheSavedKeyWhenNoneIsTyped() =
        runTest {
            val holder = holder()
            holder.fill()
            holder.save()

            holder.test()

            assertEquals(FAKE_KEY, core.asked.single().third)
        }

    @Test
    fun testDoesNotReuseASavedKeyAfterTheAddressIsEdited() =
        runTest {
            val holder = holder()
            holder.fill()
            holder.save()
            holder.change(SettingsField.Address, "https://other.example/v1")

            holder.test()

            assertEquals("", core.asked.single().third)
            assertEquals(FAKE_KEY, store.readProvider().key)
        }

    @Test
    fun aModelTheProviderDoesNotKnowFailsTheFirstLineAndSkipsTheSecond() =
        runTest {
            core.result = ModelTestResult.Failed("model not found: the provider has no model with this name.")
            val holder = holder()
            holder.fill()

            holder.test()

            val check = holder.state.value.check as SettingsCheck.Shown
            assertEquals(CheckLine(CheckStatus.Failed, "the provider has no model with this name."), check.answers)
            assertEquals(CheckLine(CheckStatus.Skipped), check.tools)
        }

    @Test
    fun aToolReasonFailsOnlyTheSecondLine() =
        runTest {
            core.result = ModelTestResult.Failed("no tool call: the model answered in text.")
            val holder = holder()
            holder.fill()

            holder.test()

            val check = holder.state.value.check as SettingsCheck.Shown
            assertEquals(CheckLine(CheckStatus.Works), check.answers)
            assertEquals(CheckStatus.Failed, check.tools.status)
        }

    @Test
    fun testNeedsAnAddressAndAModel() =
        runTest {
            val holder = holder()

            holder.test()

            assertTrue(core.asked.isEmpty())
            assertTrue(holder.state.value.addressMissing)
        }

    @Test
    fun editingAfterASaveClearsTheSavedNotice() =
        runTest {
            val holder = holder()
            holder.fill()
            holder.save()

            holder.change(SettingsField.ModelName, "x")

            assertFalse(holder.state.value.saved)
        }

    @Test
    fun anUnreadableStoreShowsItsReasonAndTryAgainReadsAgain() =
        runTest {
            val broken = FakeModelSettingsStore(backing).apply { failing = true }
            val holder = holder(broken)
            assertEquals(FakeModelSettingsStore.UNREADABLE, holder.state.value.unavailable)

            broken.failing = false
            holder.load()

            assertNull(holder.state.value.unavailable)
            assertFalse(holder.state.value.loading)
        }

    @Test
    fun aFailedSaveSaysWhyAndKeepsWhatWasTyped() =
        runTest {
            val broken = FakeModelSettingsStore(backing)
            val holder = holder(broken)
            holder.fill()
            broken.failing = true

            holder.save()

            val state = holder.state.value
            assertEquals(FakeModelSettingsStore.UNREADABLE, state.failure)
            assertFalse(state.saved)
            assertEquals(FAKE_KEY, state.keyEntry)
        }

    @Test
    fun removingAfterQueuedSaveLeavesNoKey() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val holder = SettingsHolder(store, core, this, dispatcher)
            advanceUntilIdle()
            holder.fill()
            holder.save()
            holder.removeKey()
            advanceUntilIdle()
            assertNull(store.readProvider().key)
            assertFalse(holder.state.value.keyIsSet)
        }

    @Test
    fun testUsesTheReplacementOnlyAfterSaveFinishes() =
        runTest {
            val holder = SettingsHolder(store, core, this, StandardTestDispatcher(testScheduler))
            store.saveProvider("http://127.0.0.1:8080/v1", "demo-model", "previous")
            advanceUntilIdle()
            holder.fill()
            holder.save()

            assertFalse(holder.state.value.canTest)
            holder.test()
            advanceUntilIdle()
            assertTrue(core.asked.isEmpty())
            assertEquals(SettingsCheck.None, holder.state.value.check)

            assertTrue(holder.state.value.canTest)
            holder.test()
            advanceUntilIdle()
            assertEquals(FAKE_KEY, core.asked.single().third)
        }

    @Test
    fun testWaitsUntilAllQueuedWritesFinishAndDoesNotUseTheRemovedKey() =
        runTest {
            val queuedIo = ArrayDeque<Runnable>()
            val io =
                object : CoroutineDispatcher() {
                    override fun dispatch(
                        context: CoroutineContext,
                        block: Runnable,
                    ) {
                        queuedIo.addLast(block)
                    }
                }

            fun completeNextIo() {
                runCurrent()
                queuedIo.removeFirst().run()
                runCurrent()
            }

            val holder = SettingsHolder(store, core, this, io)
            store.saveProvider("http://127.0.0.1:8080/v1", "demo-model", "previous")
            completeNextIo()
            holder.fill()
            holder.save()
            holder.removeKey()

            assertFalse(holder.state.value.canTest)
            completeNextIo() // Save completed. Remove is still queued on the I/O dispatcher.
            assertTrue(holder.state.value.saved)
            assertEquals(FAKE_KEY, store.readProvider().key)
            assertFalse(holder.state.value.canTest)
            holder.test()
            assertFalse(holder.state.value.testing)

            completeNextIo()
            assertNull(store.readProvider().key)
            assertTrue(core.asked.isEmpty())
            assertEquals(SettingsCheck.None, holder.state.value.check)

            assertTrue(holder.state.value.canTest)
            holder.test()
            completeNextIo()
            assertEquals("", core.asked.single().third)
        }

    @Test
    fun anUnreadableRecordLeavesTheFormOpenForAReplacement() =
        runTest {
            var record = "unreadable"
            val secret =
                object : SecretValue {
                    override fun read() = record

                    override fun write(secret: String) {
                        record = secret
                    }
                }
            val saved = StoredModelSettings(TextValues { null }, secret)
            val holder = holder(saved)

            assertFalse(holder.state.value.loading)
            assertNull(holder.state.value.unavailable)
            assertEquals("The saved setting cannot be opened. Enter it again.", holder.state.value.failure)
            assertFalse(holder.state.value.keyIsSet)

            holder.fill()
            holder.save()

            assertTrue(holder.state.value.saved)
            assertNull(holder.state.value.failure)
            assertEquals(FAKE_KEY, saved.readProvider().key)
        }

    @Test
    fun editingInvalidatesPendingAndFinishedTests() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val holder = SettingsHolder(store, core, this, dispatcher)
            advanceUntilIdle()
            holder.fill()
            holder.test()
            holder.change(SettingsField.ModelName, "changed")
            advanceUntilIdle()
            assertEquals(SettingsCheck.None, holder.state.value.check)
            assertFalse(holder.state.value.testing)
            holder.test()
            advanceUntilIdle()
            assertTrue(holder.state.value.check is SettingsCheck.Shown)
            holder.change(SettingsField.Key, "other")
            assertEquals(SettingsCheck.None, holder.state.value.check)
        }
}
