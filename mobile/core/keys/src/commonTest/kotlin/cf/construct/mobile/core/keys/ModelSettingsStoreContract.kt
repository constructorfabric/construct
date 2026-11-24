package cf.construct.mobile.core.keys

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * What every [ModelSettingsStore] does, whatever it keeps the values in. A subclass says how to open a
 * store, and a second open of the same phone's storage stands for a restart of the app.
 */
abstract class ModelSettingsStoreContract {
    private val address = "http://127.0.0.1:8080/v1"

    /** A store over this test's own empty storage; [reopen] reaches the same storage again. */
    abstract fun open(): ModelSettingsStore

    abstract fun reopen(): ModelSettingsStore

    @Test
    fun anEmptyPhoneHasNothingSet() {
        assertEquals(ModelSettings(), open().read())
        assertNull(open().readProvider().key)
    }

    @Test
    fun theTextSurvivesARestart() {
        open().saveProvider(address, "demo-model", null)

        assertEquals(ModelSettings(address, "demo-model", false), reopen().read())
    }

    @Test
    fun theKeySurvivesARestartAndIsOnlySaidToBeSet() {
        open().saveProvider(address, "demo-model", FAKE_KEY)

        val again = reopen()
        assertTrue(again.read().keyIsSet)
        assertEquals(FAKE_KEY, again.readProvider().key)
        assertFalse(again.read().toString().contains(FAKE_KEY))
    }

    @Test
    fun aNewKeyReplacesTheOldOne() {
        val store = open()
        store.saveProvider(address, "demo-model", FAKE_KEY)
        store.saveProvider(address, "demo-model", OTHER_KEY)

        assertEquals(OTHER_KEY, reopen().readProvider().key)
    }

    @Test
    fun aRemovedKeyIsGoneAfterARestartAndTheTextStays() {
        val store = open()
        store.saveProvider(address, "demo-model", FAKE_KEY)
        store.removeKey()

        val again = reopen()
        assertFalse(again.read().keyIsSet)
        assertNull(again.readProvider().key)
        assertEquals("demo-model", again.read().model)
    }

    @Test
    fun removingAKeyThatIsNotThereIsFine() {
        open().removeKey()

        assertFalse(open().read().keyIsSet)
    }

    @Test
    fun theKeyIsRetainedOnlyForTheSameAddress() {
        val store = open()
        store.saveProvider(address, "demo-model", FAKE_KEY)
        store.saveProvider("$address/", "renamed-model", null)

        assertEquals(FAKE_KEY, reopen().readProvider().key)

        store.saveProvider("https://different.example/v1", "renamed-model", null)
        assertNull(reopen().readProvider().key)
    }

    @Test
    fun aKeyWithAnyCharactersComesBackAsItWas() {
        val odd = "construct-model-v1:ключ \"quoted\" 'x' \n tail ✓"
        open().saveProvider(address, "demo-model", odd)

        assertEquals(odd, reopen().readProvider().key)
    }

    companion object {
        /** Recognizable on purpose: a script searches the app's files and logs for it. */
        const val FAKE_KEY = "sk-fake-0123456789-do-not-use"
        const val OTHER_KEY = "sk-other-9876543210-do-not-use"
    }
}
