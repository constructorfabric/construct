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

/** The legacy text preferences and the file of the encrypted provider record. */
const val MODEL_SETTINGS_PREFERENCES = "model_settings"
const val MODEL_KEY_FILE = "model_provider.bin"
private const val LEGACY_KEY_FILE = "model_key.bin"
private const val KEYSTORE_ALIAS = "construct.model.key"
private const val KEYSTORE_PROVIDER = "AndroidKeyStore"

/**
 * The model setting of an Android phone sits in a file under `noBackupFilesDir`,
 * which the system never backs up. The address, model and key are encrypted together
 * with an AES key that lives in the Android Keystore and cannot be taken out of it.
 */
fun androidModelSettingsStore(context: Context): ModelSettingsStore {
    val app = context.applicationContext
    val preferences = app.getSharedPreferences(MODEL_SETTINGS_PREFERENCES, Context.MODE_PRIVATE)
    val texts = TextValues { preferences.getString(it, null) }
    val directory = app.noBackupFilesDir
    return StoredModelSettings(texts, KeystoreFileSecret(File(directory, MODEL_KEY_FILE), ::keystoreAesKey)) {
        retireLegacyAndroidKey(directory)
    }
}

internal fun retireLegacyAndroidKey(directory: File) {
    for (name in listOf(LEGACY_KEY_FILE, "$LEGACY_KEY_FILE.part")) {
        val file = File(directory, name)
        if (!file.delete() && file.exists()) {
            throw ModelSettingsException("The old saved key could not be removed. Try again.")
        }
    }
}

/** The AES key of the Keystore, made on first use. It never leaves the Keystore. */
fun keystoreAesKey(): SecretKey = keystoreAesKey(KEYSTORE_ALIAS)

internal fun keystoreAesKey(alias: String): SecretKey {
    val keystore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
    (keystore.getKey(alias, null) as? SecretKey)?.let { return it }
    val spec =
        KeyGenParameterSpec
            .Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
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
            val partial = File.createTempFile(file.name, ".part", file.parentFile)
            try {
                partial.writeBytes(sealed)
                if (!partial.renameTo(file)) throw ModelSettingsException("The key could not be saved on this phone.")
            } finally {
                partial.delete()
            }
        } catch (e: ModelSettingsException) {
            throw e
        } catch (
            @Suppress("TooGenericExceptionCaught") _: Exception,
        ) {
            throw ModelSettingsException("The key could not be saved on this phone.")
        }
    }

    private companion object {
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
