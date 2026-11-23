package cf.construct.mobile.core.keys

/** One atomic secure record for a paired computer. Call off the main thread. */
class PairingSecret internal constructor(
    private val value: SecretValue,
    private val delete: () -> Unit,
) {
    @Throws(ModelSettingsException::class)
    fun read(): String? = value.read()

    @Throws(ModelSettingsException::class)
    fun save(record: String) = value.write(record)

    @Throws(ModelSettingsException::class)
    fun clear() = delete()
}

fun fakePairingSecret(): PairingSecret {
    var record: String? = null
    return PairingSecret(
        object : SecretValue {
            override fun read() = record

            override fun write(secret: String) {
                record = secret
            }
        },
    ) { record = null }
}
