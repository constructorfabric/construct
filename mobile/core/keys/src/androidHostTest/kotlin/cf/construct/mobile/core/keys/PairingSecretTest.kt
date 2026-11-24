package cf.construct.mobile.core.keys

import java.io.File
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull

/** The pairing record through the encrypted file, with an AES key of the JVM in place of the Keystore key. */
class PairingSecretTest {
    private val file = File.createTempFile("pairing", ".bin").apply { delete() }
    private val aes = newKey()

    private fun newKey(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    private fun open(key: SecretKey = aes) = PairingSecret(KeystoreFileSecret(file) { key }) { file.delete() }

    @Test
    fun aRecordWithAnIpv6AddressComesBackFromANewInstance() {
        val record = "[2001:db8::1]:47821\n${"a1".repeat(32)}\n${"e3".repeat(32)}\n1000"
        open().save(record)

        assertEquals(record, open().read())
    }

    @Test
    fun theFileHoldsNoPartOfTheRecordInTheClear() {
        open().save("192.0.2.10:47821\n${"a1".repeat(32)}\n${"e3".repeat(32)}\n1000")

        val text = String(file.readBytes(), Charsets.ISO_8859_1)

        assertFalse(text.contains("192.0.2.10"))
        assertFalse(text.contains("e3e3e3"))
    }

    @Test
    fun aChangedFileOrAnotherKeyIsRefusedWithoutQuotingAnything() {
        open().save("record")
        val key = newKey()

        val reason = assertFailsWith<ModelSettingsException> { open(key).read() }

        assertNull(reason.cause)
        assertFalse(reason.message.orEmpty().contains("record"))
    }

    @Test
    fun clearingRemovesTheRecord() {
        val secret = open()
        secret.save("record")
        secret.clear()

        assertNull(open().read())
    }
}
