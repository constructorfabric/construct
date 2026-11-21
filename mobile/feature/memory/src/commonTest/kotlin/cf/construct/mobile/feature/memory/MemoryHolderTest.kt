package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.MemoryFacts
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
    fun theFactsStayOnScreenWhileARefreshRuns() =
        runTest {
            val slow = Slow(this)
            val holder = MemoryHolder(slow, backgroundScope)
            slow.gates[0].complete(Unit)
            holder.settled()

            holder.refresh()
            runCurrent()

            assertTrue(holder.state.value.loading)
            assertEquals(FakeConstruct.seededFacts, holder.state.value.facts)
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
            // The failure goes as soon as the new read starts.
            assertNull(holder.state.value.failure)
            holder.settled()

            assertEquals(FakeConstruct.seededFacts, holder.state.value.facts)
        }

    @Test
    fun aReadThatThrowsBecomesAFailure() =
        runTest {
            val offline = Offline(this)
            offline.down = true
            offline.throws = true
            val holder = MemoryHolder(offline, backgroundScope)

            holder.settled()

            assertFalse(holder.state.value.loading)
            assertEquals(OFFLINE, holder.state.value.failure)
        }

    @Test
    fun theNewestReadWinsEvenWhenAnOlderOneEndsLast() =
        runTest {
            val slow = Slow(this)
            val holder = MemoryHolder(slow, backgroundScope)
            runCurrent()
            holder.refresh()
            runCurrent()

            // The second read ends first, then the first one is let go.
            slow.gates[1].complete(Unit)
            holder.settled()
            slow.gates[0].complete(Unit)
            runCurrent()

            assertEquals(FakeConstruct.seededFacts.drop(1), holder.state.value.facts)
            assertFalse(holder.state.value.loading)
        }

    /** The fake, which fails the facts read while [down], by an outcome or by throwing. */
    private class Offline(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        var down = false
        var throws = false

        override suspend fun getMemoryFacts(): Outcome<MemoryFacts> =
            when {
                !down -> fake.getMemoryFacts()
                throws -> error(OFFLINE)
                else -> Outcome.Failed(OFFLINE)
            }
    }

    /**
     * A [Construct] whose Nth facts read waits for `gates[N]`. The first read gives the seeded facts and
     * every later one gives them without the first fact.
     */
    private class Slow(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        val gates = List(GATES) { CompletableDeferred<Unit>() }
        private var reads = 0

        override suspend fun getMemoryFacts(): Outcome<MemoryFacts> {
            val index = reads++
            gates[index].await()
            val facts = FakeConstruct.seededFacts
            return Outcome.Ok(MemoryFacts(if (index == 0) facts else facts.drop(1), FakeConstruct.FACTS_VERSION))
        }
    }

    private companion object {
        const val OFFLINE = "offline"
        const val GATES = 4
    }
}
