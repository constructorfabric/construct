package cf.construct.mobile.feature.status

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.SmallControls
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.sync.StepProgress
import cf.construct.mobile.core.sync.SyncStep
import java.text.NumberFormat
import kotlin.math.roundToInt

private const val PERCENT = 100

/**
 * The loading screen after the first pairing: the three steps of the first copy with their counts and a bar that is
 * filled a third for each step. It has no tabs, and the app opens when the copy is whole. It shows the state it is
 * given and loads nothing.
 */
@Composable
fun FirstCopyScreen(
    state: FirstCopy,
    modifier: Modifier = Modifier,
) {
    val active = state.activeStep()
    // The empty tap handler makes the screen take every touch, also in the strips under the system bars where
    // the scroll area below does not reach, so none reaches the app under it.
    Box(
        modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .pointerInput(Unit) { detectTapGestures {} }
            .safeDrawingPadding(),
    ) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Spacing.L.dp)
                .testTag("first-copy"),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(Spacing.L.dp),
        ) {
            CircularProgressIndicator(Modifier.padding(top = Spacing.XXL.dp).size(syncLargeSpinner))
            Text(
                stringResource(R.string.first_copy_title),
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
                modifier = Modifier.semantics { heading() },
            )
            Text(
                stringResource(R.string.first_copy_text),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            CopyBar(state.fraction(), Modifier.padding(top = Spacing.M.dp))
            Text(
                stringResource(R.string.first_copy_step, state.currentNumber(), state.steps.size),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Column(Modifier.padding(top = Spacing.M.dp)) {
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                for (progress in state.steps) {
                    StepRow(progress, active = progress.step == active)
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                }
            }
        }
    }
}

@Composable
private fun CopyBar(
    fraction: Float,
    modifier: Modifier = Modifier,
) {
    val label = stringResource(R.string.first_copy_progress)
    val percent = stringResource(R.string.first_copy_percent, (fraction * PERCENT).roundToInt())
    LinearProgressIndicator(
        progress = { fraction },
        modifier =
            modifier
                .fillMaxWidth()
                .height(syncBarHeight)
                .semantics(mergeDescendants = true) {
                    contentDescription = label
                    stateDescription = percent
                },
        color = MaterialTheme.colorScheme.primary,
        trackColor = MaterialTheme.colorScheme.outlineVariant,
        strokeCap = StrokeCap.Round,
        gapSize = syncBarGap,
        drawStopIndicator = {},
    )
}

/** One step: a mark (a check when done, a spinner while it is the one under way), its name and its count. */
@Composable
private fun StepRow(
    progress: StepProgress,
    active: Boolean,
) {
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = Spacing.M.dp)
            .semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StepMark(progress.done, active)
        Text(
            stepName(progress.step),
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodyLarge,
            color = if (progress.done || active) scheme.onBackground else scheme.onSurfaceVariant,
        )
        Text(stepCount(progress, active), style = MaterialTheme.typography.bodyLarge, color = scheme.onSurfaceVariant)
    }
}

@Composable
private fun StepMark(
    done: Boolean,
    active: Boolean,
) {
    when {
        done -> {
            Icon(SyncIcons.check, contentDescription = null, Modifier.size(SmallControls.icon), tint = successColor())
        }

        active -> {
            CircularProgressIndicator(Modifier.size(SmallControls.icon), strokeWidth = SmallControls.spinnerStroke)
        }

        else -> {
            Icon(
                SyncIcons.circle,
                contentDescription = null,
                modifier = Modifier.size(SmallControls.icon),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun stepName(step: SyncStep): String =
    when (step) {
        SyncStep.MEMORY -> stringResource(R.string.first_copy_memory)
        SyncStep.CHATS -> stringResource(R.string.first_copy_chats)
        SyncStep.DASHBOARD -> stringResource(R.string.first_copy_dashboard)
    }

/**
 * What a row says it has: the amount when the step is done ("12 facts"), "12 of 40" once the step knows its total,
 * the amount so far while it is under way, "copying" before it has any, and "waiting" until its turn.
 */
@Composable
private fun stepCount(
    progress: StepProgress,
    active: Boolean,
): String {
    val total = progress.total
    return when {
        progress.done -> amount(progress.step, progress.copied)
        total != null -> stringResource(R.string.first_copy_of, number(progress.copied), number(total))
        active && progress.copied > 0 -> amount(progress.step, progress.copied)
        active -> stringResource(R.string.first_copy_copying)
        else -> stringResource(R.string.first_copy_waiting)
    }
}

@Composable
private fun amount(
    step: SyncStep,
    count: Int,
): String {
    val plural =
        when (step) {
            SyncStep.MEMORY -> R.plurals.first_copy_facts
            SyncStep.CHATS -> R.plurals.first_copy_chats_count
            SyncStep.DASHBOARD -> R.plurals.first_copy_numbers
        }
    return pluralStringResource(plural, count, number(count))
}

private fun number(count: Int): String = NumberFormat.getIntegerInstance().format(count)
