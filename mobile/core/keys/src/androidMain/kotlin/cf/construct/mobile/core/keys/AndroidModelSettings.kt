package cf.construct.mobile.core.keys

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** The preferences file of the address and the model name, and the file of the encrypted key. */
const val MODEL_SETTINGS_PREFERENCES = "model_settings"
const val MODEL_KEY_FILE = "model_key.bin"
private const val KEYSTORE_ALIAS = "construct.model.key"
private const val KEYSTORE_PROVIDER = "AndroidKeyStore"

/**
 * The model setting of an Android phone. The address and the model name sit in a private preferences
 * file. The key sits in a file under `noBackupFilesDir`, which the system never backs up, encrypted
 * with an AES key that lives in the Android Keystore and cannot be taken out of it.
 */
fun androidModelSettingsStore(context: Context): ModelSettingsStore {
    val app = context.applicationContext
    val preferences = app.getSharedPreferences(MODEL_SETTINGS_PREFERENCES, Context.MODE_PRIVATE)
    val texts =
        object : TextValues {
            override fun get(name: String) = preferences.getString(name, null)

            override fun put(
                name: String,
                value: String,
            ) {
                // commit(), not apply(): a failed write has to be seen, and a restart right after must find it.
                if (!preferences.edit().putString(name, value).commit()) {
                    throw ModelSettingsException("The setting could not be saved on this phone.")
                }
            }
        }
    return StoredModelSettings(texts, KeystoreFileSecret(File(app.noBackupFilesDir, MODEL_KEY_FILE), ::keystoreAesKey))
}

/** The AES key of the Keystore, made on first use. It never leaves the Keystore. */
fun keystoreAesKey(): SecretKey {
    val keystore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
    (keystore.getKey(KEYSTORE_ALIAS, null) as? SecretKey)?.let { return it }
    val spec =
        KeyGenParameterSpec
            .Builder(KEYSTORE_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(AES_BITS)
            .build()
    return KeyGenerator
        .getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        .apply { init(spec) }
        .generateKey()
}

private const val AES_BITS = 256
private const val IV_BYTES = 12
private const val TAG_BITS = 128

/**
 * One secret in [file], encrypted with AES-GCM under the key [key] gives. The file holds the random
 * nonce, then the encrypted bytes. A key that cannot be opened (a restored phone whose Keystore key is
 * gone, a changed file) is reported with a plain reason and replaced by saving a new one.
 */
class KeystoreFileSecret(
    private val file: File,
    private val key: () -> SecretKey,
) : SecretValue {
    override fun isSet() = file.isFile

    override fun read(): String? {
        if (!file.isFile) return null
        try {
            val bytes = file.readBytes()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, bytes, 0, IV_BYTES))
            return String(cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES), Charsets.UTF_8)
        } catch (
            @Suppress("TooGenericExceptionCaught") _: Exception,
        ) {
            // The cause is dropped: an exception can quote what was being opened.
            throw ModelSettingsException("The saved key cannot be opened. Enter it again.")
        }
    }

    override fun write(secret: String) {
        try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val sealed = cipher.iv + cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
            // Written aside and moved over the old file, so a failed write never leaves half a key.
            val partial = File(file.parentFile, file.name + ".part")
            partial.writeBytes(sealed)
            if (!partial.renameTo(file)) {
                partial.delete()
                throw ModelSettingsException("The key could not be saved on this phone.")
            }
        } catch (e: ModelSettingsException) {
            throw e
        } catch (
            @Suppress("TooGenericExceptionCaught") _: Exception,
        ) {
            throw ModelSettingsException("The key could not be saved on this phone.")
        }
    }

    override fun delete() {
        if (file.exists() && !file.delete()) throw ModelSettingsException("The key could not be removed.")
    }

    private companion object {
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
