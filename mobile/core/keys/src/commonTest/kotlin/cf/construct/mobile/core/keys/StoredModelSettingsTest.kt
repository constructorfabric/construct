package cf.construct.mobile.core.keys

/** The shared store over text and secret storage that live in memory: what it adds to them is the contract. */
class StoredModelSettingsTest : ModelSettingsStoreContract() {
    private val texts = mutableMapOf<String, String>()
    private var secret: String? = null

    private val textValues =
        object : TextValues {
            override fun get(name: String) = texts[name]

            override fun put(
                name: String,
                value: String,
            ) {
                texts[name] = value
            }
        }
    private val secretValue =
        object : SecretValue {
            override fun isSet() = secret != null

            override fun read() = secret

            override fun write(secret: String) {
                this@StoredModelSettingsTest.secret = secret
            }

            override fun delete() {
                secret = null
            }
        }

    override fun open(): ModelSettingsStore = StoredModelSettings(textValues, secretValue)

    override fun reopen(): ModelSettingsStore = StoredModelSettings(textValues, secretValue)
}
