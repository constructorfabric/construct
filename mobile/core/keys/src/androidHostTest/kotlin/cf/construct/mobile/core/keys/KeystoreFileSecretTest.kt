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
    private val texts = mutableMapOf<String, String>()
    private val aes = newKey()

    private fun newKey(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    private fun secret(key: SecretKey = aes) = KeystoreFileSecret(File(folder, "model_key.bin"), { key })

    private fun store(key: SecretKey = aes) =
        StoredModelSettings(
            object : TextValues {
                override fun get(name: String) = texts[name]

                override fun put(
                    name: String,
                    value: String,
                ) {
                    texts[name] = value
                }
            },
            secret(key),
        )

    override fun open(): ModelSettingsStore = store()

    override fun reopen(): ModelSettingsStore = store()

    @Test
    fun theFileHoldsNoPartOfTheKeyInTheClear() {
        secret().write(FAKE_KEY)

        val bytes = File(folder, "model_key.bin").readBytes()

        assertFalse(String(bytes, Charsets.ISO_8859_1).contains("fake"))
        assertFalse(String(bytes, Charsets.ISO_8859_1).contains(FAKE_KEY.take(6)))
    }

    @Test
    fun theSameKeyEncryptsToDifferentBytesEachTime() {
        val secret = secret()
        secret.write(FAKE_KEY)
        val first = File(folder, "model_key.bin").readBytes()
        secret.write(FAKE_KEY)

        assertFalse(first.contentEquals(File(folder, "model_key.bin").readBytes()))
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
        val file = File(folder, "model_key.bin")
        file.writeBytes(file.readBytes().also { it[it.size - 1] = (it.last() + 1).toByte() })

        assertFailsWith<ModelSettingsException> { secret.read() }
        assertTrue(secret.isSet())

        secret.write(OTHER_KEY)
        assertEquals(OTHER_KEY, secret.read())
    }

    @Test
    fun noPartOfAFailedWriteIsLeftBehind() {
        val secret = KeystoreFileSecret(File(folder, "model_key.bin")) { error("no key") }

        assertFailsWith<ModelSettingsException> { secret.write(FAKE_KEY) }

        assertFalse(secret.isSet())
        assertEquals(emptyList(), folder.list().orEmpty().toList())
    }
}
