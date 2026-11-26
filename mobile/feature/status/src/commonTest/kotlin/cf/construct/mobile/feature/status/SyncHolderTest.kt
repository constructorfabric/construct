package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.database.SyncRecord
import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncResult
import cf.construct.mobile.core.sync.SyncStep
import cf.construct.mobile.core.sync.Syncer
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

@OptIn(ExperimentalCoroutinesApi::class)
class SyncHolderTest {
    /** A sync that ends when the test says so, and reports the progress the test gives it. */
    private class FakeSyncer(
        var record: SyncRecord = SyncRecord(computer = "A", firstCopyDone = true, syncedAt = "2026-11-10T12:00:00Z"),
    ) : Syncer {
        var started = 0
        var cancelled = 0
        var onStart: ((StepProgress) -> Unit) -> Unit = {}
        var next: CompletableDeferred<SyncResult>? = null
        var result: SyncResult = SyncResult.Done
        var after: () -> Unit = {}
        var recordFailure: Exception? = null

        override suspend fun sync(progress: (StepProgress) -> Unit): SyncResult {
            started++
            onStart(progress)
            val wait = next
            try {
                val outcome = wait?.await() ?: result
                if (outcome == SyncResult.Done) after()
                return outcome
            } catch (cancel: kotlinx.coroutines.CancellationException) {
                cancelled++
                throw cancel
            }
        }

        override fun record(): SyncRecord = recordFailure?.let { throw it } ?: record
    }

    private fun TestScope.holder(syncer: Syncer) =
        SyncHolder(
            syncer,
            backgroundScope,
            interval = 60.seconds,
            justSyncedTime = 3.seconds,
            storageDispatcher = UnconfinedTestDispatcher(testScheduler),
        )

    /** Every state the holder has been in. */
    private fun TestScope.watch(holder: SyncHolder): MutableList<SyncState> {
        val seen = mutableListOf<SyncState>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { holder.state.collect { seen += it } }
        return seen
    }

    @Test
    fun theFirstCopyShowsItsStepsAndTheirCountsAndEndsWithoutANotice() =
        runTest {
            val syncer = FakeSyncer(SyncRecord(computer = "A"))
            val gate = CompletableDeferred<SyncResult>()
            syncer.next = gate
            var report: (StepProgress) -> Unit = {}
            syncer.onStart = { report = it }
            val holder = holder(syncer)
            val seen = watch(holder)
            runCurrent()
            assertEquals(FirstCopy(), holder.state.value.firstCopy)

            holder.setForeground(true)
            runCurrent()
            assertEquals(
                true,
                holder.state.value.firstCopy
                    ?.running,
            )
            report(StepProgress(SyncStep.MEMORY, copied = 4))
            report(StepProgress(SyncStep.MEMORY, 7, 7, done = true))
            report(StepProgress(SyncStep.CHATS, copied = 2))
            assertEquals(
                listOf(
                    StepProgress(SyncStep.MEMORY, 7, 7, true),
                    StepProgress(SyncStep.CHATS, 2),
                    StepProgress(SyncStep.DASHBOARD),
                ),
                holder.state.value.firstCopy
                    ?.steps,
            )
            assertEquals(SyncNotice.None, holder.state.value.notice)

            syncer.record = SyncRecord(computer = "A", firstCopyDone = true, syncedAt = "2026-11-10T12:05:00Z")
            gate.complete(SyncResult.Done)
            runCurrent()
            assertNull(holder.state.value.firstCopy)
            assertEquals("2026-11-10T12:05:00Z", holder.state.value.lastSyncedAt)
            assertTrue(seen.all { it.notice == SyncNotice.None }, "the loading screen is not also a banner")
        }

    @Test
    fun theSyncWhenTheAppOpensShowsSyncingAndThenJustSyncedForAFewSeconds() =
        runTest {
            val syncer = FakeSyncer()
            val gate = CompletableDeferred<SyncResult>()
            syncer.next = gate
            val holder = holder(syncer)
            runCurrent()
            assertNull(holder.state.value.firstCopy)

            holder.setForeground(true)
            runCurrent()
            assertEquals(SyncNotice.Syncing, holder.state.value.notice)

            gate.complete(SyncResult.Done)
            runCurrent()
            assertEquals(SyncNotice.JustSynced, holder.state.value.notice)
            advanceTimeBy(2.seconds)
            assertEquals(SyncNotice.JustSynced, holder.state.value.notice)
            advanceTimeBy(1.seconds)
            runCurrent()
            assertEquals(SyncNotice.None, holder.state.value.notice)
        }

    @Test
    fun syncNowIsASyncThePersonWaitsFor() =
        runTest {
            val syncer = FakeSyncer()
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            advanceTimeBy(10.seconds)
            val seen = watch(holder)
            runCurrent()

            holder.syncNow()
            runCurrent()

            assertEquals(2, syncer.started)
            assertTrue(SyncNotice.Syncing in seen.map { it.notice } || SyncNotice.JustSynced in seen.map { it.notice })
            assertEquals(SyncNotice.JustSynced, holder.state.value.notice)
        }

    @Test
    fun theCheckEveryMinuteIsSilentTenTimesInARow() =
        runTest {
            val syncer = FakeSyncer()
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            advanceTimeBy(4.seconds)
            val seen = watch(holder)
            runCurrent()
            seen.clear()

            repeat(10) {
                // New data every time: the record's time moves, as a sync that copied something does.
                syncer.record = syncer.record.copy(syncedAt = "2026-11-10T12:0$it:00Z")
                advanceTimeBy(60.seconds)
                runCurrent()
            }

            assertEquals(11, syncer.started)
            assertTrue(seen.all { it.notice == SyncNotice.None }, "no notice for a check nobody waits for")
        }

    @Test
    fun aComputerThatIsNotReachableIsToldOnceAndNotAtEveryCheck() =
        runTest {
            val syncer = FakeSyncer()
            syncer.result = SyncResult.Missed(ComputerLink.Unreachable)
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)
            assertEquals("2026-11-10T12:00:00Z", holder.state.value.lastSyncedAt)

            val seen = watch(holder)
            runCurrent()
            seen.clear()
            repeat(5) {
                advanceTimeBy(60.seconds)
                runCurrent()
            }
            assertEquals(5, syncer.started - 1)
            assertTrue(seen.isEmpty(), "the notice is not announced again by a check that finds the same")
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)

            // It works again: the notice goes, and a later loss is told again.
            syncer.result = SyncResult.Done
            advanceTimeBy(60.seconds)
            runCurrent()
            assertEquals(SyncNotice.None, holder.state.value.notice)
            syncer.result = SyncResult.Missed(ComputerLink.Unreachable)
            advanceTimeBy(60.seconds)
            runCurrent()
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)
        }

    @Test
    fun tryAgainShowsSyncingAndThenTheNoticeAgainWhenItFails() =
        runTest {
            val syncer = FakeSyncer()
            syncer.result = SyncResult.Missed(ComputerLink.NoNetwork)
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            val seen = watch(holder)
            runCurrent()
            seen.clear()

            val gate = CompletableDeferred<SyncResult>()
            syncer.next = gate
            holder.syncNow()
            runCurrent()
            assertEquals(SyncNotice.Syncing, holder.state.value.notice)
            gate.complete(SyncResult.Missed(ComputerLink.NoNetwork))
            runCurrent()
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)
            assertEquals(listOf(SyncNotice.Syncing, SyncNotice.NotReachable), seen.map { it.notice })
        }

    @Test
    fun aPhoneTheComputerUnpairedOrThatMustUpdateTellsNothingHere() =
        runTest {
            val syncer = FakeSyncer()
            val holder = holder(syncer)
            for (link in listOf(
                ComputerLink.NotPaired,
                ComputerLink.PhoneUpdate,
                ComputerLink.ComputerUpdate,
                ComputerLink.NetworkPermission,
            )) {
                syncer.result = SyncResult.Missed(link)
                holder.setForeground(false)
                runCurrent()
                holder.setForeground(true)
                runCurrent()
                assertEquals(SyncNotice.None, holder.state.value.notice, "for $link")
            }
            assertNotNull(holder.state.value.lastSyncedAt)
        }

    @Test
    fun theNoticeOfAComputerThatWasNotReachableGoesWhenThePhoneIsUnpaired() =
        runTest {
            val syncer = FakeSyncer()
            syncer.result = SyncResult.Missed(ComputerLink.Unreachable)
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)

            syncer.result = SyncResult.Missed(ComputerLink.NotPaired)
            holder.syncNow()
            runCurrent()
            assertEquals(SyncNotice.None, holder.state.value.notice)
        }

    @Test
    fun aCheckNowAfterUnpairClearsTheNoticeWithoutShowingSyncingAndStartsNoSecondSync() =
        runTest {
            val syncer = FakeSyncer()
            syncer.result = SyncResult.Missed(ComputerLink.Unreachable)
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            assertEquals(SyncNotice.NotReachable, holder.state.value.notice)
            val seen = watch(holder)
            runCurrent()
            seen.clear()

            syncer.result = SyncResult.Missed(ComputerLink.NotPaired)
            holder.checkNow()
            runCurrent()
            assertEquals(SyncNotice.None, holder.state.value.notice)
            assertTrue(seen.none { it.notice == SyncNotice.Syncing }, "nobody waits for this check")

            // While a sync runs, the check does not start another one.
            syncer.next = CompletableDeferred()
            holder.checkNow()
            runCurrent()
            val started = syncer.started
            holder.checkNow()
            runCurrent()
            assertEquals(started, syncer.started)
        }

    @Test
    fun nothingSyncsInTheBackgroundAndASyncThatRunsIsStopped() =
        runTest {
            val syncer = FakeSyncer()
            syncer.next = CompletableDeferred()
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            assertEquals(SyncNotice.Syncing, holder.state.value.notice)

            holder.setForeground(false)
            runCurrent()
            assertEquals(1, syncer.cancelled)
            assertEquals(SyncNotice.None, holder.state.value.notice)
            advanceTimeBy(300.seconds)
            runCurrent()
            assertEquals(1, syncer.started)

            syncer.next = null
            holder.setForeground(true)
            runCurrent()
            assertEquals(2, syncer.started)
        }

    @Test
    fun aCopyThatLostItsComputerShowsTheFirstCopyAgain() =
        runTest {
            val syncer = FakeSyncer()
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()
            assertNull(holder.state.value.firstCopy)

            // Another computer was paired: the copy was emptied and waits for its first copy.
            syncer.record = SyncRecord(computer = "B")
            syncer.next = CompletableDeferred()
            holder.syncNow()
            runCurrent()
            assertNotNull(holder.state.value.firstCopy)
        }

    @Test
    fun aCopyThatCannotBeReadDoesNotStopTheChecksAndTheNextOneShowsWhatItFinds() =
        runTest {
            val syncer = FakeSyncer(SyncRecord(computer = "A", firstCopyDone = true, syncedAt = "2026-11-10T12:00:00Z"))
            syncer.recordFailure = IllegalStateException("disk full")
            syncer.result = SyncResult.Cut
            val holder = holder(syncer)
            holder.setForeground(true)
            runCurrent()

            assertEquals(1, syncer.started)
            assertEquals(SyncNotice.None, holder.state.value.notice)
            assertEquals(false, holder.state.value.known)

            advanceTimeBy(60.seconds + 1.milliseconds)
            runCurrent()
            assertEquals(2, syncer.started, "the check a minute later still runs")

            syncer.recordFailure = null
            syncer.result = SyncResult.Done
            syncer.record = SyncRecord(computer = "A", firstCopyDone = true, syncedAt = "2026-11-10T12:05:00Z")
            advanceTimeBy(60.seconds + 1.milliseconds)
            runCurrent()
            assertEquals(3, syncer.started)
            assertEquals(true, holder.state.value.known)
            assertEquals("2026-11-10T12:05:00Z", holder.state.value.lastSyncedAt)
        }
}
