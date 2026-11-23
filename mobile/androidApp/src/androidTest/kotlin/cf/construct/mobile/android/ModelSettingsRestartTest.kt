package cf.construct.mobile.android

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test

/** Run each phase in a separate instrumentation process to check persistence across restarts. */
class ModelSettingsRestartTest {
    private val address = "http://127.0.0.1:9/v1"

    @Test
    fun platformStoreContract() {
        val app = InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as ConstructApp
        val store = app.modelSettings
        val odd = "sk-ключ \"quoted\" 'x' \n tail ✓"
        try {
            store.removeKey()
            store.saveProvider("", "", null)
            assertFalse(store.read().keyIsSet)
            assertNull(store.readProvider().key)
            store.removeKey()
            store.saveProvider(address, "demo-model", odd)
            assertEquals(odd, app.modelSettings.readProvider().key)
            assertTrue(app.modelSettings.read().keyIsSet)
            val setting = app.modelSettings.read()
            assertFalse(setting.toString().contains(odd))
            store.saveProvider(address, "demo-model", "replacement")
            assertEquals("replacement", app.modelSettings.readProvider().key)
            store.removeKey()
            assertNull(app.modelSettings.readProvider().key)
            assertEquals("demo-model", app.modelSettings.read().model)
        } finally {
            store.removeKey()
            store.saveProvider("", "", null)
        }
    }

    @Test
    fun settingsSurviveRestartAndRemoval() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val phase = InstrumentationRegistry.getArguments().getString("restartCheck")
        assumeTrue("only the restart check sets this", phase != null)
        val store = (instrumentation.targetContext.applicationContext as ConstructApp).modelSettings
        when (phase) {
            "save" -> {
                store.saveProvider(address, "demo-model", "sk-restart-test-0123456789-do-not-use")
            }

            "remove" -> {
                assertEquals("demo-model", store.read().model)
                assertEquals("sk-restart-test-0123456789-do-not-use", store.readProvider().key)
                store.removeKey()
            }

            "verify" -> {
                assertNull(store.readProvider().key)
                assertFalse(store.read().keyIsSet)
                store.saveProvider("", "", null)
            }

            else -> {
                error("Unknown restart check phase")
            }
        }
    }
}
