package cf.construct.mobile.feature.status

import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncStep
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class FirstCopyProgressTest {
    private fun copy(
        memory: StepProgress = StepProgress(SyncStep.MEMORY),
        chats: StepProgress = StepProgress(SyncStep.CHATS),
        dashboard: StepProgress = StepProgress(SyncStep.DASHBOARD),
    ) = FirstCopy(steps = listOf(memory, chats, dashboard))

    @Test
    fun theBarIsEmptyWhenNothingIsCopied() {
        assertEquals(0f, copy().fraction())
    }

    @Test
    fun aStepWhoseTotalIsNotKnownCountsNothing() {
        assertEquals(0f, copy(memory = StepProgress(SyncStep.MEMORY, copied = 5)).fraction())
    }

    @Test
    fun aStepFillsItsThirdAsItIsCopied() {
        val half = copy(memory = StepProgress(SyncStep.MEMORY, copied = 6, total = 12))
        assertEquals(1f / 6, half.fraction(), 0.0001f)
    }

    @Test
    fun aStepThatIsDoneFillsItsWholeThird() {
        val first = copy(memory = StepProgress(SyncStep.MEMORY, copied = 12, total = 12, done = true))
        assertEquals(1f / 3, first.fraction(), 0.0001f)
    }

    @Test
    fun aDoneStepCountsWholeEvenWithoutATotal() {
        val first = copy(dashboard = StepProgress(SyncStep.DASHBOARD, done = true))
        assertEquals(1f / 3, first.fraction(), 0.0001f)
    }

    @Test
    fun aCountPastItsTotalDoesNotOverfillTheBar() {
        val over = copy(memory = StepProgress(SyncStep.MEMORY, copied = 20, total = 10))
        assertEquals(1f / 3, over.fraction(), 0.0001f)
    }

    @Test
    fun aTotalOfZeroCountsNothing() {
        assertEquals(0f, copy(chats = StepProgress(SyncStep.CHATS, copied = 0, total = 0)).fraction())
    }

    @Test
    fun theBarIsFullWhenAllStepsAreDone() {
        val all =
            copy(
                StepProgress(SyncStep.MEMORY, done = true),
                StepProgress(SyncStep.CHATS, done = true),
                StepProgress(SyncStep.DASHBOARD, done = true),
            )
        assertEquals(1f, all.fraction(), 0.0001f)
    }

    @Test
    fun noStepsFillNothing() {
        assertEquals(0f, FirstCopy(steps = emptyList()).fraction())
    }

    @Test
    fun theCurrentStepIsTheFirstThatIsNotDone() {
        assertEquals(1, copy().currentNumber())
        assertEquals(2, copy(memory = StepProgress(SyncStep.MEMORY, done = true)).currentNumber())
        val two = copy(StepProgress(SyncStep.MEMORY, done = true), StepProgress(SyncStep.CHATS, done = true))
        assertEquals(3, two.currentNumber())
    }

    @Test
    fun theCurrentStepIsTheLastWhenAllAreDone() {
        val all =
            copy(
                StepProgress(SyncStep.MEMORY, done = true),
                StepProgress(SyncStep.CHATS, done = true),
                StepProgress(SyncStep.DASHBOARD, done = true),
            )
        assertEquals(3, all.currentNumber())
    }

    @Test
    fun theActiveStepIsTheFirstThatIsNotDoneEvenBeforeItReportsACount() {
        assertEquals(SyncStep.MEMORY, copy().activeStep())
        assertEquals(SyncStep.CHATS, copy(memory = StepProgress(SyncStep.MEMORY, done = true)).activeStep())
    }

    @Test
    fun noStepIsActiveWhenAllAreDone() {
        val all =
            copy(
                StepProgress(SyncStep.MEMORY, done = true),
                StepProgress(SyncStep.CHATS, done = true),
                StepProgress(SyncStep.DASHBOARD, done = true),
            )
        assertNull(all.activeStep())
    }
}
