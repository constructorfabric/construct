package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MemoryHolderTest {
    /** Waits until the read that is running has ended. */
    private suspend fun MemoryHolder.settled() {
        state.first { !it.loading }
    }

    @Test
    fun startsLoadingThenShowsTheFactsOfTheFake() =
        runTest {
            val holder = MemoryHolder(FakeConstruct(backgroundScope), backgroundScope)
            assertTrue(holder.state.value.loading)
            assertTrue(
                holder.state.value.facts
                    .isEmpty(),
            )

            holder.settled()

            val state = holder.state.value
            assertFalse(state.loading)
            assertEquals(FakeConstruct.seededFacts, state.facts)
            assertNull(state.failure)
        }

    @Test
    fun refreshShowsWhatChangedInTheFake() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val holder = MemoryHolder(fake, backgroundScope)
            holder.settled()

            fake.deleteMemoryFact("f1")
            holder.refresh()
            holder.settled()

            assertEquals(FakeConstruct.seededFacts.drop(1), holder.state.value.facts)
        }

    @Test
    fun aFailedReadKeepsTheFactsAndSaysWhy() =
        runTest {
            val offline = Offline(this)
            val holder = MemoryHolder(offline, backgroundScope)
            holder.settled()
            offline.down = true

            holder.refresh()
            holder.settled()

            val state = holder.state.value
            assertFalse(state.loading)
            assertEquals(FakeConstruct.seededFacts, state.facts)
            assertEquals(OFFLINE, state.failure)
        }

    @Test
    fun aLaterGoodReadClearsTheFailure() =
        runTest {
            val offline = Offline(this)
            offline.down = true
            val holder = MemoryHolder(offline, backgroundScope)
            holder.settled()
            assertEquals(OFFLINE, holder.state.value.failure)

            offline.down = false
            holder.refresh()
            holder.settled()

            assertNull(holder.state.value.failure)
            assertEquals(FakeConstruct.seededFacts, holder.state.value.facts)
        }

    /** The fake, which fails the facts read while [down]. */
    private class Offline(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        var down = false

        override suspend fun getMemoryFacts(): Outcome<MemoryFacts> =
            if (down) Outcome.Failed(OFFLINE) else fake.getMemoryFacts()
    }

    private companion object {
        const val OFFLINE = "offline"
    }
}
