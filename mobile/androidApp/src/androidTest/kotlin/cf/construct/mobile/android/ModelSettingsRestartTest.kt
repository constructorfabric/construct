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
    @Test
    fun platformStoreContract() {
        val app = InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as ConstructApp
        val store = app.modelSettings
        val odd = "sk-ключ \"quoted\" 'x' \n tail ✓"
        try {
            store.removeKey()
            store.saveText("", "")
            assertFalse(store.read().keyIsSet)
            assertNull(store.readKey())
            store.removeKey()
            store.saveKey(odd)
            store.saveText("http://127.0.0.1:9/v1", "demo-model")
            assertEquals(odd, app.modelSettings.readKey())
            assertTrue(app.modelSettings.read().keyIsSet)
            val setting = app.modelSettings.read()
            assertFalse(setting.toString().contains(odd))
            store.saveKey("replacement")
            assertEquals("replacement", app.modelSettings.readKey())
            store.removeKey()
            assertNull(app.modelSettings.readKey())
            assertEquals("demo-model", app.modelSettings.read().model)
        } finally {
            store.removeKey()
            store.saveText("", "")
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
                store.saveText("http://127.0.0.1:9/v1", "demo-model")
                store.saveKey("sk-restart-test-0123456789-do-not-use")
            }

            "remove" -> {
                assertEquals("demo-model", store.read().model)
                assertEquals("sk-restart-test-0123456789-do-not-use", store.readKey())
                store.removeKey()
            }

            "verify" -> {
                assertNull(store.readKey())
                assertFalse(store.read().keyIsSet)
                store.saveText("", "")
            }

            else -> {
                error("Unknown restart check phase")
            }
        }
    }
}
