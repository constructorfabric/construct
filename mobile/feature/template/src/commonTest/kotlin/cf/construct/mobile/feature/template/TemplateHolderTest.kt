package cf.construct.mobile.feature.template

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.ConversationSummary
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
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
    fun theItemsStayOnScreenWhileARefreshRuns() =
        runTest {
            val slow = Slow(this)
            val holder = TemplateHolder(slow, backgroundScope)
            slow.gates[0].complete(Unit)
            holder.state.first { !it.loading }

            holder.refresh()
            runCurrent()

            assertTrue(holder.state.value.loading)
            assertEquals(seededTitles, holder.state.value.items)
        }

    @Test
    fun theNewestReadWinsEvenWhenAnOlderOneEndsLast() =
        runTest {
            val slow = Slow(this)
            val holder = TemplateHolder(slow, backgroundScope)
            runCurrent()
            holder.refresh()
            runCurrent()

            // The second read ends first, then the first one is let go.
            slow.gates[1].complete(Unit)
            holder.state.first { !it.loading }
            slow.gates[0].complete(Unit)
            runCurrent()

            assertEquals(seededTitles.drop(1), holder.state.value.items)
            assertFalse(holder.state.value.loading)
            assertNull(holder.state.value.failure)
        }

    /** The fake, which fails the conversations read while [down]. */
    private class Offline(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        var down = false

        override suspend fun listConversations(): Outcome<List<ConversationSummary>> =
            if (down) Outcome.Failed(OFFLINE) else fake.listConversations()
    }

    /**
     * A [Construct] whose Nth conversations read waits for `gates[N]`. The first read gives the seeded
     * conversations and every later one gives them without the first.
     */
    private class Slow(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        val gates = List(GATES) { CompletableDeferred<Unit>() }
        private var reads = 0

        override suspend fun listConversations(): Outcome<List<ConversationSummary>> {
            val index = reads++
            gates[index].await()
            val all = (fake.listConversations() as Outcome.Ok).value
            return Outcome.Ok(if (index == 0) all else all.drop(1))
        }
    }

    private companion object {
        const val OFFLINE = "offline"
        const val GATES = 4
    }
}
