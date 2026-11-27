package cf.construct.mobile.feature.dashboard

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.core.model.GroupMinutes

/**
 * The sessions behind one row of a table, the Material counterpart of the list the iPhone pushes. It shows what the
 * copy holds: each session with its times, its own minutes, its tool, project and ticket, and the sum that is the
 * row. A row the computer sent no sessions for says so.
 */
@Composable
internal fun DashboardSessionsPage(
    shown: WidgetState,
    row: GroupMinutes,
    sessions: RowSessions,
    modifier: Modifier = Modifier,
    onBack: () -> Unit,
) {
    BackHandler(onBack = onBack)
    Column(modifier.fillMaxSize().testTag("dashboard-sessions")) {
        TextButton(onClick = onBack, modifier = Modifier.testTag("dashboard-sessions-back")) { Text("Back") }
        Column(
            Modifier.padding(horizontal = Spacing.L.dp),
            verticalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        ) {
            Text(
                row.key,
                Modifier.semantics { heading() },
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                "${DashboardText.minutes(row.minutes)} · ${DashboardText.sessions(row.sessions)}",
                Modifier.testTag("dashboard-sessions-total"),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
            if (!sessions.sent) {
                Text(
                    DashboardText.sessionsNotSent(shown.span),
                    Modifier.testTag("dashboard-sessions-not-sent"),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
            } else if (sessions.sharedMinutes) {
                Text(
                    DashboardText.SHARED_MINUTES,
                    Modifier.testTag("dashboard-sessions-shared"),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        LazyColumn(Modifier.fillMaxWidth().padding(Spacing.L.dp)) {
            items(sessions.lines.size) { index ->
                SessionLineRow(sessions.lines[index], shown.span == DashboardSpan.WEEK, index)
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun SessionLineRow(
    line: SessionLine,
    withDate: Boolean,
    index: Int,
) {
    Column(
        Modifier.fillMaxWidth().padding(vertical = Spacing.S.dp).testTag("dashboard-session-$index"),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Spacing.M.dp)) {
            val time = if (withDate) "${line.date} ${line.start} – ${line.end}" else "${line.start} – ${line.end}"
            Text(time, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            Text(
                DashboardText.minutes(line.minutes),
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.SemiBold,
            )
        }
        Text(
            "${line.tool} · ${line.project} · ${line.ticket}",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.labelMedium,
        )
    }
}
