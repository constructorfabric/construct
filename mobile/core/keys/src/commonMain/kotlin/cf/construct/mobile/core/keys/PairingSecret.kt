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

/** A record in memory for tests. While [failing] is true, saving and removing throw like a broken store. */
fun fakePairingSecret(failing: () -> Boolean = { false }): PairingSecret {
    var record: String? = null
    return PairingSecret(
        object : SecretValue {
            override fun read() = record

            override fun write(secret: String) {
                if (failing()) throw ModelSettingsException("The pairing could not be saved on this phone.")
                record = secret
            }
        },
    ) {
        if (failing()) throw ModelSettingsException("The pairing could not be removed from this phone.")
        record = null
    }
}
