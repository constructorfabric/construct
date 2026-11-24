package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.LinkSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals

class ConnectionStatusTest {
    @Test
    fun everyLinkStateHasItsOwnKind() {
        val kinds = ComputerLink.entries.map { ConnectionStatus.from(LinkSnapshot(it, 0)).kind }
        assertEquals(ComputerLink.entries.size, kinds.toSet().size)
    }

    @Test
    fun theUpdateStatesSayWhatToUpdate() {
        assertEquals(
            "Update this app to connect to the computer",
            ConnectionStatus.from(LinkSnapshot(ComputerLink.PhoneUpdate, 0)).kind.title,
        )
        assertEquals(
            "Update Construct on the computer to connect",
            ConnectionStatus.from(LinkSnapshot(ComputerLink.ComputerUpdate, 0)).kind.title,
        )
    }

    @Test
    fun theReasonsForNoConnectionTellTheCause() {
        assertEquals(
            "This phone has no network connection.",
            ConnectionStatus.from(LinkSnapshot(ComputerLink.NoNetwork, 0)).kind.detail,
        )
        assertEquals(
            "Allow access to connect to your computer.",
            ConnectionStatus.from(LinkSnapshot(ComputerLink.NetworkPermission, 0)).kind.detail,
        )
    }

    @Test
    fun theEndpointAndTheLastContactAreCarried() {
        val status = ConnectionStatus.from(LinkSnapshot(ComputerLink.Unreachable, 1234, "192.0.2.10:4242"))
        assertEquals(1234L, status.lastContactMillis)
        assertEquals("192.0.2.10:4242", status.endpoint)
    }

    @Test
    fun onlyAStateThatAnotherTryCanChangeOffersTryAgain() {
        val retryable =
            ComputerLink.entries
                .filter { ConnectionStatus.from(LinkSnapshot(it, 0)).kind.canRetry }
                .toSet()
        assertEquals(
            setOf(
                ComputerLink.NoNetwork,
                ComputerLink.NetworkPermission,
                ComputerLink.Unreachable,
                ComputerLink.StorageError,
            ),
            retryable,
        )
    }
}
