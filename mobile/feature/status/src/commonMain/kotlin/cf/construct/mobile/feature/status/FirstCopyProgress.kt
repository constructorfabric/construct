package cf.construct.mobile.feature.status

import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncStep

private const val WHOLE = 1f

/**
 * How full the bar of the loading screen is: a third for each step, and for a step that is not done the part of it
 * that is copied, once its total is known. A step that does not know its total counts nothing yet.
 */
fun FirstCopy.fraction(): Float {
    if (steps.isEmpty()) return 0f
    return steps.sumOf { it.part().toDouble() }.toFloat() / steps.size
}

private fun StepProgress.part(): Float {
    val all = total
    return when {
        done -> WHOLE
        all == null || all <= 0 -> 0f
        else -> (copied.toFloat() / all).coerceIn(0f, WHOLE)
    }
}

/** The number of the step being made, from 1: the first that is not done, or the last one when all are. */
fun FirstCopy.currentNumber(): Int {
    val first = steps.indexOfFirst { !it.done }
    return (if (first >= 0) first else maxOf(steps.size - 1, 0)) + 1
}

/**
 * The step under way. The steps are made one after the other, so it is the first that is not done, even before it has
 * reported a count. It is null when all are done.
 */
fun FirstCopy.activeStep(): SyncStep? = steps.firstOrNull { !it.done }?.step
