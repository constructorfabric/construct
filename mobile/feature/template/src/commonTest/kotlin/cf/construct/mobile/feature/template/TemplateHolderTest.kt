package cf.construct.mobile.feature.template

import cf.construct.mobile.core.construct.FakeConstruct
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class TemplateHolderTest {
    @Test
    fun startsLoadingThenShowsWhatTheFakeHolds() =
        runTest {
            val holder = TemplateHolder(FakeConstruct(backgroundScope), backgroundScope)
            assertTrue(holder.state.value.loading)

            holder.state.first { !it.loading }

            val state = holder.state.value
            assertFalse(state.loading)
            assertEquals(
                FakeConstruct.seededConversations.keys
                    .map { it.title }
                    .toSet(),
                state.items.toSet(),
            )
        }
}
