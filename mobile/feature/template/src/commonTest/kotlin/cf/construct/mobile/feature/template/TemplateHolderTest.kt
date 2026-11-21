package cf.construct.mobile.feature.template

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class TemplateHolderTest {
    private val seededTitles =
        FakeConstruct.seededConversations.keys
            .sortedByDescending { it.time }
            .map { it.title }

    @Test
    fun startsLoadingThenShowsTheItemsOfTheFakeInTheirOrder() =
        runTest {
            val holder = TemplateHolder(FakeConstruct(backgroundScope), backgroundScope)
            assertTrue(holder.state.value.loading)

            holder.state.first { !it.loading }

            assertEquals(seededTitles, holder.state.value.items)
            assertNull(holder.state.value.failure)
        }

    @Test
    fun aFailedReadKeepsTheItemsAndSaysWhy() =
        runTest {
            val offline = Offline(this)
            val holder = TemplateHolder(offline, backgroundScope)
            holder.state.first { !it.loading }
            offline.down = true

            holder.refresh()
            holder.state.first { !it.loading }

            assertEquals(seededTitles, holder.state.value.items)
            assertEquals(OFFLINE, holder.state.value.failure)
        }

    @Test
    fun aReadThatThrowsBecomesAFailure() =
        runTest {
            val offline = Offline(this)
            offline.down = true
            offline.throws = true
            val holder = TemplateHolder(offline, backgroundScope)

            holder.state.first { !it.loading }

            assertFalse(holder.state.value.loading)
            assertEquals(OFFLINE, holder.state.value.failure)
        }

    /** The fake, which fails the conversations read while [down], by an outcome or by throwing. */
    private class Offline(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        var down = false
        var throws = false

        override suspend fun listConversations(): Outcome<List<ConversationSummary>> =
            when {
                !down -> fake.listConversations()
                throws -> error(OFFLINE)
                else -> Outcome.Failed(OFFLINE)
            }
    }

    private companion object {
        const val OFFLINE = "offline"
    }
}
