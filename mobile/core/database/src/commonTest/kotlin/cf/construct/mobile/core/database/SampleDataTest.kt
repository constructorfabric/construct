package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.MemoryFacts
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SampleDataTest {
    private val store = CopyStore(newInMemoryDriver())

    @Test
    fun anEmptyCopyGetsTheSeededFacts() {
        assertTrue(store.seedWithSampleData())

        assertEquals(MemoryFacts(FakeConstruct.seededFacts, FakeConstruct.FACTS_VERSION), store.facts())
    }

    @Test
    fun aCopyThatHasAProfileIsLeftAsItIs() {
        store.saveFacts(MemoryFacts(emptyList(), 7L))

        assertFalse(store.seedWithSampleData())

        assertEquals(MemoryFacts(emptyList(), 7L), store.facts())
    }

    @Test
    fun seedingTwiceChangesNothing() {
        store.seedWithSampleData()

        assertFalse(store.seedWithSampleData())

        assertEquals(FakeConstruct.seededFacts, store.facts().facts)
    }
}
