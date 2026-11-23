package cf.construct.mobile.core.keys

import kotlin.test.Test
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse

class FakeModelSettingsStoreTest : ModelSettingsStoreContract() {
    private val backing = FakeBacking()

    override fun open(): ModelSettingsStore = FakeModelSettingsStore(backing)

    override fun reopen(): ModelSettingsStore = FakeModelSettingsStore(backing)

    @Test
    fun aFailingFakeThrowsAPlainReasonThatHoldsNoKey() {
        val store = FakeModelSettingsStore(backing).apply { failing = true }

        val reason = assertFailsWith<ModelSettingsException> { store.saveKey(FAKE_KEY) }

        assertFalse(reason.message.orEmpty().contains(FAKE_KEY))
    }
}
