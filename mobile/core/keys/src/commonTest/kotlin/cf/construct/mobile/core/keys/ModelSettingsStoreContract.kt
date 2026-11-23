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
    /** A store over this test's own empty storage; [reopen] reaches the same storage again. */
    abstract fun open(): ModelSettingsStore

    abstract fun reopen(): ModelSettingsStore

    @Test
    fun anEmptyPhoneHasNothingSet() {
        assertEquals(ModelSettings(), open().read())
        assertNull(open().readKey())
    }

    @Test
    fun theTextSurvivesARestart() {
        open().saveText("http://127.0.0.1:8080/v1", "demo-model")

        assertEquals(ModelSettings("http://127.0.0.1:8080/v1", "demo-model", false), reopen().read())
    }

    @Test
    fun theKeySurvivesARestartAndIsOnlySaidToBeSet() {
        open().saveKey(FAKE_KEY)

        val again = reopen()
        assertTrue(again.read().keyIsSet)
        assertEquals(FAKE_KEY, again.readKey())
        assertFalse(again.read().toString().contains(FAKE_KEY))
    }

    @Test
    fun aNewKeyReplacesTheOldOne() {
        val store = open()
        store.saveKey(FAKE_KEY)
        store.saveKey(OTHER_KEY)

        assertEquals(OTHER_KEY, reopen().readKey())
    }

    @Test
    fun aRemovedKeyIsGoneAfterARestartAndTheTextStays() {
        val store = open()
        store.saveText("http://127.0.0.1:8080/v1", "demo-model")
        store.saveKey(FAKE_KEY)
        store.removeKey()

        val again = reopen()
        assertFalse(again.read().keyIsSet)
        assertNull(again.readKey())
        assertEquals("demo-model", again.read().model)
    }

    @Test
    fun removingAKeyThatIsNotThereIsFine() {
        open().removeKey()

        assertFalse(open().read().keyIsSet)
    }

    @Test
    fun savingTheTextLeavesTheKeyAlone() {
        val store = open()
        store.saveKey(FAKE_KEY)
        store.saveText("http://127.0.0.1:8080/v1", "demo-model")

        assertEquals(FAKE_KEY, reopen().readKey())
    }

    @Test
    fun aKeyWithAnyCharactersComesBackAsItWas() {
        val odd = "sk-ключ \"quoted\" 'x' \n tail ✓"
        open().saveKey(odd)

        assertEquals(odd, reopen().readKey())
    }

    companion object {
        /** Recognizable on purpose: a script searches the app's files and logs for it. */
        const val FAKE_KEY = "sk-fake-0123456789-do-not-use"
        const val OTHER_KEY = "sk-other-9876543210-do-not-use"
    }
}
