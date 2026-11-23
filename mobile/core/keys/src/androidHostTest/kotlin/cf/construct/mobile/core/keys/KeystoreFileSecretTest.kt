package cf.construct.mobile.core.keys

import java.io.File
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The encrypted file with an AES key of the JVM in place of the Keystore key (the Keystore exists only on a
 * phone; the emulator test runs the same checks with the real one). The shared contract runs over it too.
 */
class KeystoreFileSecretTest : ModelSettingsStoreContract() {
    private val folder = File.createTempFile("keys", "").apply { delete() }.also { it.mkdirs() }
    private val recordFile = File(folder, MODEL_KEY_FILE)
    private val aes = newKey()

    private fun newKey(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    private fun secret(key: SecretKey = aes) = KeystoreFileSecret(recordFile) { key }

    private fun store(key: SecretKey = aes) =
        StoredModelSettings(
            TextValues { null },
            secret(key),
        )

    override fun open(): ModelSettingsStore = store()

    override fun reopen(): ModelSettingsStore = store()

    @Test
    fun theFileHoldsNoPartOfTheKeyInTheClear() {
        secret().write(FAKE_KEY)

        val bytes = recordFile.readBytes()

        assertFalse(String(bytes, Charsets.ISO_8859_1).contains("fake"))
        assertFalse(String(bytes, Charsets.ISO_8859_1).contains(FAKE_KEY.take(6)))
    }

    @Test
    fun theSameKeyEncryptsToDifferentBytesEachTime() {
        val secret = secret()
        secret.write(FAKE_KEY)
        val first = recordFile.readBytes()
        secret.write(FAKE_KEY)

        assertFalse(first.contentEquals(recordFile.readBytes()))
    }

    @Test
    fun aKeyThatCannotBeOpenedSaysSoWithoutQuotingAnything() {
        secret().write(FAKE_KEY)

        val reason = assertFailsWith<ModelSettingsException> { secret(newKey()).read() }

        assertEquals("The saved key cannot be opened. Enter it again.", reason.message)
        assertNull(reason.cause)
    }

    @Test
    fun aChangedFileIsRefusedAndASavedKeyReplacesIt() {
        val secret = secret()
        secret.write(FAKE_KEY)
        recordFile.writeBytes(recordFile.readBytes().also { it[it.size - 1] = (it.last() + 1).toByte() })

        assertFailsWith<ModelSettingsException> { secret.read() }
        assertTrue(recordFile.isFile)

        secret.write(OTHER_KEY)
        assertEquals(OTHER_KEY, secret.read())
    }

    @Test
    fun noPartOfAFailedWriteIsLeftBehind() {
        val secret = KeystoreFileSecret(recordFile) { error("no key") }

        assertFailsWith<ModelSettingsException> { secret.write(FAKE_KEY) }

        assertEquals(emptyList(), folder.list().orEmpty().toList())
    }

    @Test
    fun removesLegacyKeyFilesWithoutOpeningThemAndKeepsProviderSettings() {
        val legacy = File(folder, "model_key.bin")
        val legacyPartial = File(folder, "model_key.bin.part")
        KeystoreFileSecret(legacy, ::newKey).write("construct-model-v1:legacy-key")
        legacy.copyTo(legacyPartial)
        val unrelated = File(folder, "unrelated.bin").apply { writeText("keep") }
        val preferences = mapOf("model_address" to "https://example.com/v1", "model_name" to "old")
        val migrated = StoredModelSettings(TextValues(preferences::get), secret()) { retireLegacyAndroidKey(folder) }

        assertEquals(ModelSettings("https://example.com/v1", "old", keyIsSet = false), migrated.read())
        assertFalse(legacy.exists())
        assertFalse(legacyPartial.exists())

        migrated.saveProvider("https://example.com/v1", "new", OTHER_KEY)
        assertEquals(OTHER_KEY, reopen().readProvider().key)
        migrated.removeKey()
        assertNull(reopen().readProvider().key)
        assertEquals("new", reopen().read().model)
        assertEquals("keep", unrelated.readText())
    }

    @Test
    fun replacesAnUnreadableEncryptedProviderWithAnExplicitKey() {
        open().saveProvider("https://old.example", "old", FAKE_KEY)
        val replacementKey = newKey()
        val replacementStore = store(replacementKey)
        assertFailsWith<UnreadableModelSettingsException> { replacementStore.read() }

        replacementStore.saveProvider("https://new.example", "new", OTHER_KEY)

        val saved = store(replacementKey).readProvider()
        assertEquals("https://new.example", saved.address)
        assertEquals(OTHER_KEY, saved.key)
    }
}
