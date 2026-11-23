package cf.construct.mobile.feature.settings

import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.chat.ModelTestResult
import cf.construct.mobile.core.keys.FakeModelSettingsStore
import cf.construct.mobile.core.keys.ModelSettingsStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SettingsWriteRaceTest {
    @Test
    fun removeWaitsForAnInFlightSave() {
        val saved = CountDownLatch(1)
        val release = CountDownLatch(1)
        val removed = CountDownLatch(1)
        val delegate = FakeModelSettingsStore()
        val store =
            object : ModelSettingsStore by delegate {
                override fun saveKey(key: String) {
                    saved.countDown()
                    check(release.await(5, TimeUnit.SECONDS))
                    delegate.saveKey(key)
                }

                override fun removeKey() {
                    delegate.removeKey()
                    removed.countDown()
                }
            }
        val core =
            object : ChatCore {
                override fun fixedRequestBody(toolsFile: String) = error("not used")

                override fun testModel(
                    baseUrl: String,
                    model: String,
                    apiKey: String,
                ): ModelTestResult = error("not used")
            }
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val io = Executors.newFixedThreadPool(2).asCoroutineDispatcher()
        try {
            val holder = SettingsHolder(store, core, scope, io)
            runBlocking { withTimeout(5_000) { holder.state.first { !it.loading } } }
            holder.change(SettingsField.Address, "http://127.0.0.1:9/v1")
            holder.change(SettingsField.ModelName, "model")
            holder.change(SettingsField.Key, "fake-key")
            holder.save()
            assertTrue(saved.await(5, TimeUnit.SECONDS))
            holder.removeKey()
            assertFalse(removed.await(200, TimeUnit.MILLISECONDS))
            release.countDown()
            assertTrue(removed.await(5, TimeUnit.SECONDS))
            assertNull(delegate.readKey())
        } finally {
            release.countDown()
            scope.cancel()
            io.close()
        }
    }
}
