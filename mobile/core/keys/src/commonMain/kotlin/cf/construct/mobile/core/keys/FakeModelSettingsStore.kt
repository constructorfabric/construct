package cf.construct.mobile.core.keys

/** What a [FakeModelSettingsStore] keeps; two fakes over one backing are one phone before and after a restart. */
class FakeBacking {
    internal var provider = ProviderSettings("", "", null)
}

/** A store in memory, for tests and previews. [failing] makes every call throw [ModelSettingsException]. */
class FakeModelSettingsStore(
    private val backing: FakeBacking = FakeBacking(),
) : ModelSettingsStore {
    var failing = false

    private fun check() {
        if (failing) throw ModelSettingsException(UNREADABLE)
    }

    override fun read(): ModelSettings = readProvider().publicSettings()

    override fun readProvider(): ProviderSettings {
        check()
        return backing.provider
    }

    override fun saveProvider(
        address: String,
        model: String,
        key: String?,
    ): ModelSettings {
        check()
        val saved = ProviderSettings(address, model, key ?: backing.provider.keyFor(address))
        backing.provider = saved
        return saved.publicSettings()
    }

    override fun removeKey() {
        val current = readProvider()
        backing.provider = ProviderSettings(current.address, current.model, null)
    }

    companion object {
        const val UNREADABLE = "The stored settings cannot be read."
    }
}
