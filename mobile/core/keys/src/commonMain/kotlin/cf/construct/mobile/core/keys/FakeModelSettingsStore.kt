package cf.construct.mobile.core.keys

/** What a [FakeModelSettingsStore] keeps; two fakes over one backing are one phone before and after a restart. */
class FakeBacking {
    var address = ""
    var model = ""
    var key: String? = null
}

/** A store in memory, for tests and previews. [failing] makes every call throw [ModelSettingsException]. */
class FakeModelSettingsStore(
    private val backing: FakeBacking = FakeBacking(),
) : ModelSettingsStore {
    var failing = false

    private fun check() {
        if (failing) throw ModelSettingsException(UNREADABLE)
    }

    override fun read(): ModelSettings {
        check()
        return ModelSettings(backing.address, backing.model, backing.key != null)
    }

    override fun saveText(
        address: String,
        model: String,
    ) {
        check()
        backing.address = address
        backing.model = model
    }

    override fun readKey(): String? {
        check()
        return backing.key
    }

    override fun saveKey(key: String) {
        check()
        backing.key = key
    }

    override fun removeKey() {
        check()
        backing.key = null
    }

    companion object {
        const val UNREADABLE = "The stored settings cannot be read."
    }
}
