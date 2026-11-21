package cf.construct.mobile.android

import cf.construct.mobile.core.database.CopyStore
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class SeedCopyTest {
    @Test
    fun aReleaseBuildLeavesTheCopyEmpty() {
        val store = CopyStore(newInMemoryDriver())

        seedCopy(store)

        assertNull(store.memoryVersion())
        assertEquals(emptyList(), store.facts().facts)
    }
}
