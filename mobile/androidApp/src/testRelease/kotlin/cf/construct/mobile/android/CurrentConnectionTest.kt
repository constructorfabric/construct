package cf.construct.mobile.android

import kotlin.test.Test
import kotlin.test.assertEquals

/** Pins the constant, so that changing what a build type reports is a deliberate edit. */
class CurrentConnectionTest {
    @Test
    fun aReleaseBuildIsNotConnectedUntilPairingExists() {
        assertEquals(ConnectionState.NotConnected, currentConnection())
    }
}
