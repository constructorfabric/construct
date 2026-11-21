package cf.construct.mobile.android

import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.database.CopyStore
import kotlin.test.Test
import kotlin.test.assertEquals

class SeedCopyTest {
    @Test
    fun aDebugBuildFillsAnEmptyCopyWithTheSampleFacts() {
        val store = CopyStore(newInMemoryDriver())

        seedCopy(store)

        assertEquals(FakeConstruct.seededFacts, store.facts().facts)
    }

    @Test
    fun aCopyThatCannotBeWrittenDoesNotCrashTheLaunch() {
        val driver = newInMemoryDriver()
        val store = CopyStore(driver)
        driver.close()

        seedCopy(store)
    }
}
