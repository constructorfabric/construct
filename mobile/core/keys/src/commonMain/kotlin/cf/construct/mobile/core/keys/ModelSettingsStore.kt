package cf.construct.mobile.core.keys

/**
 * The model setting the person made: where the model is, its name, and whether a key is saved. The
 * key itself is not part of it, so no screen state, log line or copy can hold it by accident.
 */
data class ModelSettings(
    val address: String = "",
    val model: String = "",
    val keyIsSet: Boolean = false,
)

/**
 * Keeps the model setting on the phone. The address and the model name sit in the phone's own storage.
 * The key sits in the Keychain on an iPhone and under a Keystore key on Android, and is in no copy of
 * the app's data and in no backup.
 *
 * Calls read and write storage and wait for it: call them off the main thread. A call that cannot
 * finish throws [ModelSettingsException] (Swift sees it as an error to catch); its text is plain
 * and never holds the key.
 */
interface ModelSettingsStore {
    @Throws(ModelSettingsException::class)
    fun read(): ModelSettings

    @Throws(ModelSettingsException::class)
    fun saveText(
        address: String,
        model: String,
    )

    /** The whole key for a call to the model, or null when none is saved. It is never shown. */
    @Throws(ModelSettingsException::class)
    fun readKey(): String?

    @Throws(ModelSettingsException::class)
    fun saveKey(key: String)

    @Throws(ModelSettingsException::class)
    fun removeKey()
}

/** A storage call that failed, with a fixed plain reason. */
class ModelSettingsException(
    message: String,
) : Exception(message)

/** Small named text values in the phone's own storage: [name] to text. */
interface TextValues {
    fun get(name: String): String?

    fun put(
        name: String,
        value: String,
    )
}

/** One secret in the phone's secure storage. */
interface SecretValue {
    fun isSet(): Boolean

    fun read(): String?

    fun write(secret: String)

    fun delete()
}

/** A [ModelSettingsStore] over the text storage and the secure storage of a platform. */
class StoredModelSettings(
    private val texts: TextValues,
    private val secret: SecretValue,
) : ModelSettingsStore {
    override fun read() =
        ModelSettings(
            address = texts.get(ADDRESS).orEmpty(),
            model = texts.get(MODEL).orEmpty(),
            keyIsSet = secret.isSet(),
        )

    override fun saveText(
        address: String,
        model: String,
    ) {
        texts.put(ADDRESS, address)
        texts.put(MODEL, model)
    }

    override fun readKey() = secret.read()

    override fun saveKey(key: String) = secret.write(key)

    override fun removeKey() = secret.delete()

    private companion object {
        const val ADDRESS = "model_address"
        const val MODEL = "model_name"
    }
}
