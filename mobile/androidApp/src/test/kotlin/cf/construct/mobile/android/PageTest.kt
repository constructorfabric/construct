package cf.construct.mobile.android

import kotlin.test.Test
import kotlin.test.assertEquals

/** Deliberate change-detectors: the order and the names of the five pages are the app's navigation. */
class PageTest {
    @Test
    fun theNavigationBarHasTheFivePagesInOrder() {
        assertEquals(
            listOf("Chat", "Memory", "Dashboard", "Glasshouse", "Settings"),
            Page.entries.map { it.name },
        )
    }

    @Test
    fun everyPageHasItsOwnTitleAndItsOwnIcon() {
        assertEquals(
            Page.entries.size,
            Page.entries
                .map { it.title }
                .toSet()
                .size,
        )
        assertEquals(
            Page.entries.size,
            Page.entries
                .map { it.icon.name }
                .toSet()
                .size,
        )
    }
}
