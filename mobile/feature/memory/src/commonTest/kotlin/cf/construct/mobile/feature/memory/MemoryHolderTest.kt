package cf.construct.mobile.feature.memory

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.MemoryFacts
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map
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
    fun aChangeInTheFakeShowsWithoutARefresh() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val holder = MemoryHolder(fake, backgroundScope)
            holder.settled()

            fake.deleteMemoryFact("f1")
            runCurrent()

            val state = holder.state.value
            assertEquals(FakeConstruct.seededFacts.drop(1), state.facts)
            assertFalse(state.loading)
            assertNull(state.failure)
        }

    @Test
    fun erasingEverythingEmptiesTheListWithoutARefresh() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val holder = MemoryHolder(fake, backgroundScope)
            holder.settled()

            fake.eraseEverything()
            runCurrent()

            assertTrue(
                holder.state.value.facts
                    .isEmpty(),
            )
            assertFalse(holder.state.value.loading)
        }

    @Test
    fun refreshWatchesAgainAndShowsWhatTheFakeHoldsThen() =
        runTest {
            val fake = FakeConstruct(backgroundScope)
            val holder = MemoryHolder(fake, backgroundScope)
            holder.settled()

            holder.refresh()
            assertTrue(holder.state.value.loading)
            holder.settled()

            assertEquals(FakeConstruct.seededFacts, holder.state.value.facts)
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
    fun aFailureInTheStreamKeepsTheFactsAndALaterGoodValueClearsItWithoutARefresh() =
        runTest {
            val pushed = Pushed(this)
            val holder = MemoryHolder(pushed, backgroundScope)
            holder.settled()

            pushed.facts.value = Outcome.Failed(OFFLINE)
            runCurrent()
            assertEquals(OFFLINE, holder.state.value.failure)
            assertEquals(FakeConstruct.seededFacts, holder.state.value.facts)
            assertFalse(holder.state.value.loading)

            pushed.facts.value = Outcome.Ok(MemoryFacts(FakeConstruct.seededFacts.drop(2), FakeConstruct.FACTS_VERSION))
            runCurrent()
            assertNull(holder.state.value.failure)
            assertEquals(FakeConstruct.seededFacts.drop(2), holder.state.value.facts)
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

    /** The fake, whose stream fails while [down]. */
    private class Offline(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        var down = false

        override val memoryFacts: Flow<Outcome<MemoryFacts>> =
            fake.memoryFacts.map { if (down) Outcome.Failed(OFFLINE) else it }
    }

    /**
     * A [Construct] whose Nth watch of the facts waits for `gates[N]`. The first watch gives the seeded
     * facts and every later one gives them without the first fact.
     */
    private class Slow(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        val gates = List(GATES) { CompletableDeferred<Unit>() }
        private var watches = 0

        override val memoryFacts: Flow<Outcome<MemoryFacts>> =
            flow {
                val index = watches++
                gates[index].await()
                val facts = FakeConstruct.seededFacts
                emit(Outcome.Ok(MemoryFacts(if (index == 0) facts else facts.drop(1), FakeConstruct.FACTS_VERSION)))
            }
    }

    /** A [Construct] whose stream gives whatever the test puts in [facts], starting with the seeded facts. */
    private class Pushed(
        scope: TestScope,
        private val fake: FakeConstruct = FakeConstruct(scope.backgroundScope),
    ) : Construct by fake {
        val facts =
            MutableStateFlow<Outcome<MemoryFacts>>(
                Outcome.Ok(MemoryFacts(FakeConstruct.seededFacts, FakeConstruct.FACTS_VERSION)),
            )

        override val memoryFacts: Flow<Outcome<MemoryFacts>> = facts
    }

    private companion object {
        const val OFFLINE = "offline"
        const val GATES = 4
    }
}
