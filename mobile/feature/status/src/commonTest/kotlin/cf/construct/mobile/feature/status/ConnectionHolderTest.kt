package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.LinkSnapshot
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalCoroutinesApi::class)
class ConnectionHolderTest {
    /** A monitor that publishes one snapshot when it starts and then waits until it is cancelled. */
    private class Monitor(
        private val snapshot: LinkSnapshot,
    ) {
        var started = 0
        var running = 0

        suspend fun watch(publish: (LinkSnapshot) -> Unit) {
            started++
            running++
            try {
                publish(snapshot)
                awaitCancellation()
            } finally {
                running--
            }
        }
    }

    @Test
    fun itWatchesOnlyInTheForegroundAndShowsWhatTheMonitorPublishes() =
        runTest {
            val monitor = Monitor(LinkSnapshot(ComputerLink.PhoneUpdate, 0, "192.0.2.10:4242"))
            val holder = ConnectionHolder(monitor::watch, backgroundScope)
            runCurrent()
            assertEquals(0, monitor.started)
            assertEquals(ConnectionStatusKind.Checking, holder.state.value.kind)

            holder.setForeground(true)
            runCurrent()
            assertEquals(1, monitor.running)
            assertEquals(ConnectionStatusKind.PhoneUpdate, holder.state.value.kind)
            assertEquals("192.0.2.10:4242", holder.state.value.endpoint)

            holder.setForeground(false)
            runCurrent()
            assertEquals(0, monitor.running)

            holder.setForeground(true)
            runCurrent()
            assertEquals(2, monitor.started)
            assertEquals(1, monitor.running)
        }

    @Test
    fun eachTryAgainRestartsTheMonitorWhileInTheForegroundAndOnlyThen() =
        runTest {
            val monitor = Monitor(LinkSnapshot(ComputerLink.Unreachable, 0))
            val holder = ConnectionHolder(monitor::watch, backgroundScope)
            holder.tryAgain()
            runCurrent()
            assertEquals(0, monitor.started)

            holder.setForeground(true)
            runCurrent()
            assertEquals(1, monitor.started)

            holder.tryAgain()
            runCurrent()
            holder.tryAgain()
            runCurrent()
            assertEquals(3, monitor.started)
            assertEquals(1, monitor.running)
        }
}
