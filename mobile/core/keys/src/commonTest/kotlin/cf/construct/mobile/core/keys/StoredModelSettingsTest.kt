package cf.construct.mobile.core.keys

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull

/** The shared store over text and secret storage that live in memory: what it adds to them is the contract. */
class StoredModelSettingsTest : ModelSettingsStoreContract() {
    private val texts = mutableMapOf<String, String>()
    private var secret: String? = null
    private var rejectWrites = false

    private val textValues = TextValues(texts::get)
    private val secretValue =
        object : SecretValue {
            override fun read() = secret

            override fun write(secret: String) {
                if (rejectWrites) throw ModelSettingsException("write refused")
                this@StoredModelSettingsTest.secret = secret
            }
        }

    override fun open(): ModelSettingsStore = StoredModelSettings(textValues, secretValue)

    override fun reopen(): ModelSettingsStore = StoredModelSettings(textValues, secretValue)

    @Test
    fun aFailedProviderChangeLeavesThePreviousAddressAndKeyTogether() {
        val previous = "https://first.example/v1"
        val next = "https://second.example/v1"
        open().saveProvider(previous, "first", "fake-first")
        rejectWrites = true
        assertFailsWith<ModelSettingsException> { open().saveProvider(next, "second", "fake-second") }
        val saved = reopen().readProvider()
        assertEquals(previous, saved.address)
        assertEquals("first", saved.model)
        assertEquals("fake-first", saved.key)

        rejectWrites = false
        open().saveProvider(next, "second", null)
        val changed = reopen().readProvider()
        assertEquals(next, changed.address)
        assertNull(changed.key)
    }

    @Test
    fun readsLegacyTextUntilTheUserSavesTheFirstProviderRecord() {
        val address = "https://example.com/v1"
        texts["model_address"] = address
        texts["model_name"] = "old-model"
        val previous = open().readProvider()
        assertEquals(address, previous.address)
        assertEquals("old-model", previous.model)
        assertNull(previous.key)

        open().saveProvider("$address/", "new-model", "replacement")
        texts["model_address"] = "https://different.example/v1"
        val saved = reopen().readProvider()
        assertEquals("$address/", saved.address)
        assertEquals("new-model", saved.model)
        assertEquals("replacement", saved.key)
    }

    @Test
    fun aCorruptProviderRecordRequiresAnExplicitReplacement() {
        secret = "corrupt"
        val store = open()
        assertFailsWith<UnreadableModelSettingsException> { store.read() }
        assertFailsWith<UnreadableModelSettingsException> {
            store.saveProvider("https://example.com/v1", "new-model", null)
        }
        assertEquals("corrupt", secret)
        store.saveProvider("https://example.com/v1", "new-model", "replacement")
        assertEquals("replacement", reopen().readProvider().key)
    }
}
