package cf.construct.mobile.core.keys

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive

/**
 * The model setting the person made: where the model is, its name, and whether a key is saved. The
 * key itself is not part of it, so no screen state, log line or copy can hold it by accident.
 */
data class ModelSettings(
    val address: String = "",
    val model: String = "",
    val keyIsSet: Boolean = false,
)

/** One provider snapshot for a request. Keep it out of screen state and diagnostic output. */
class ProviderSettings(
    val address: String,
    val model: String,
    val key: String?,
) {
    internal fun publicSettings() = ModelSettings(address, model, key != null)

    fun keyFor(address: String): String? = key.takeIf { sameProvider(this.address, address) }
}

private fun sameProvider(
    first: String,
    second: String,
): Boolean = first.trim().trimEnd('/') == second.trim().trimEnd('/')

/**
 * Keeps the model setting as one record in the Keychain on iPhone or under a Keystore key on Android.
 * The key is in no copy of the app's data and in no backup.
 *
 * Calls read and write storage and wait for it: call them off the main thread. A call that cannot
 * finish throws [ModelSettingsException] (Swift sees it as an error to catch); its text is plain
 * and never holds the key.
 */
interface ModelSettingsStore {
    @Throws(ModelSettingsException::class)
    fun readProvider(): ProviderSettings

    /** Save all fields together. A blank key field keeps the saved key only at the same address. */
    @Throws(ModelSettingsException::class)
    fun saveProvider(
        address: String,
        model: String,
        key: String?,
    ): ModelSettings

    @Throws(ModelSettingsException::class)
    fun read(): ModelSettings

    @Throws(ModelSettingsException::class)
    fun removeKey()
}

/** A storage call that failed, with a fixed plain reason. */
open class ModelSettingsException(
    message: String,
) : Exception(message)

/** Saved provider data could not be opened. The form can still accept a replacement. */
class UnreadableModelSettingsException : ModelSettingsException("The saved setting cannot be opened. Enter it again.")

/** Read settings written to preferences by earlier releases. */
fun interface TextValues {
    fun get(name: String): String?
}

/** One secret in the phone's secure storage. */
interface SecretValue {
    fun read(): String?

    fun write(secret: String)
}

/** A [ModelSettingsStore] over the text storage and the secure storage of a platform. */
class StoredModelSettings(
    private val texts: TextValues,
    private val secret: SecretValue,
    private val retireLegacyKey: () -> Unit = {},
) : ModelSettingsStore {
    override fun readProvider(): ProviderSettings {
        retireLegacyKey()
        return readSavedProvider()
    }

    private fun readSavedProvider(): ProviderSettings =
        try {
            // Earlier releases wrote text and credentials separately. Keep the text until the
            // first save. The old key has no recorded provider binding and is never read here.
            secret.read()?.let(::decode)
                ?: ProviderSettings(texts.get(ADDRESS).orEmpty(), texts.get(MODEL).orEmpty(), null)
        } catch (_: ModelSettingsException) {
            throw UnreadableModelSettingsException()
        }

    override fun read(): ModelSettings = readProvider().publicSettings()

    override fun saveProvider(
        address: String,
        model: String,
        key: String?,
    ): ModelSettings {
        retireLegacyKey()
        val saved = ProviderSettings(address, model, key ?: readSavedProvider().keyFor(address))
        write(saved)
        return saved.publicSettings()
    }

    override fun removeKey() {
        val current = readProvider()
        write(ProviderSettings(current.address, current.model, null))
    }

    private fun write(provider: ProviderSettings) {
        val record =
            JsonArray(
                listOf(JsonPrimitive(provider.address), JsonPrimitive(provider.model), JsonPrimitive(provider.key)),
            )
        secret.write(record.toString())
    }

    private fun decode(record: String): ProviderSettings =
        runCatching {
            val fields = Json.parseToJsonElement(record).jsonArray
            check(fields.size == RECORD_FIELD_COUNT)
            ProviderSettings(
                fields[0].jsonPrimitive.content,
                fields[1].jsonPrimitive.content,
                fields[2].jsonPrimitive.contentOrNull,
            )
        }.getOrElse { throw UnreadableModelSettingsException() }

    private companion object {
        const val ADDRESS = "model_address"
        const val MODEL = "model_name"
        const val RECORD_FIELD_COUNT = 3
    }
}
