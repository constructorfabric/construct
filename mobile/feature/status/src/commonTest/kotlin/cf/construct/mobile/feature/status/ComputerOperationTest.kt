package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.PairResult
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ComputerOperationTest {
    @Test
    fun aPairedResultPairsWithoutAMessage() {
        val operation = PairResult.Paired.toOperation()
        assertTrue(operation.paired)
        assertNull(operation.message)
        assertFalse(operation.busy)
    }

    @Test
    fun everyOtherResultShowsItsOwnMessageAndNeverPairs() {
        PairResult.entries.filter { it != PairResult.Paired }.forEach { result ->
            val operation = result.toOperation()
            assertFalse(operation.paired, result.name)
            assertNotNull(operation.message, result.name)
            assertEquals(result.message, operation.message, result.name)
        }
    }

    @Test
    fun theUpdateResultsReachTheScreenWithTheirText() {
        assertEquals("Update this app to connect to the computer.", PairResult.PhoneUpdate.toOperation().message)
        assertEquals("Update Construct on the computer to connect.", PairResult.ComputerUpdate.toOperation().message)
    }

    @Test
    fun onlyAMissingPermissionAsksForThePermission() {
        val asking = PairResult.entries.filter { it.toOperation().needsNetworkPermission }
        assertEquals(listOf(PairResult.NetworkPermission), asking)
    }

    @Test
    fun onlyAnUnusableCodeAsksForAFreshOne() {
        val asking = PairResult.entries.filter { it.toOperation().needsFreshCode }
        assertEquals(listOf(PairResult.FreshCode, PairResult.Refused), asking)
    }
}
