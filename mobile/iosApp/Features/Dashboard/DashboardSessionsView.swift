import ConstructShared
import SwiftUI

/// The sessions behind one row of a table, pushed on the page: each with its times, its own minutes, its tool,
/// project and ticket, and the sum that is the row. A row the computer sent no sessions for says so.
struct DashboardSessionsView: View {
    let span: DashboardSpan
    let row: GroupMinutes
    let sessions: RowSessions

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.medium) {
                Text("\(DashboardText.minutes(row.minutes)) · \(DashboardText.sessions(row.sessions))")
                    .font(.system(size: ConstructTheme.TextSize.secondary))
                    .foregroundColor(ConstructTheme.color(.mutedForeground))
                    .accessibilityIdentifier("dashboard-sessions-total")
                if !sessions.sent {
                    note(DashboardText.sessionsNotSent(span), id: "dashboard-sessions-not-sent")
                } else if sessions.sharedMinutes {
                    note(DashboardText.sharedMinutes, id: "dashboard-sessions-shared")
                }
                ForEach(Array(sessions.lines.enumerated()), id: \.offset) { index, line in
                    SessionLineView(line: line, withDate: span == .week)
                        .accessibilityIdentifier("dashboard-session-\(index)")
                    Divider()
                }
            }
            .padding(ConstructTheme.Space.large)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(ConstructTheme.color(.background))
        .navigationTitle(row.key)
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("dashboard-sessions")
    }

    private func note(_ text: String, id: String) -> some View {
        Text(text)
            .font(.system(size: ConstructTheme.TextSize.secondary))
            .foregroundColor(ConstructTheme.color(.mutedForeground))
            .accessibilityIdentifier(id)
    }
}

private struct SessionLineView: View {
    let line: SessionLine
    let withDate: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
            HStack(alignment: .firstTextBaseline) {
                Text(withDate ? "\(line.date) \(line.start) – \(line.end)" : "\(line.start) – \(line.end)")
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
                Spacer(minLength: ConstructTheme.Space.medium)
                Text(DashboardText.minutes(line.minutes))
                    .font(.system(size: ConstructTheme.TextSize.body, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
            }
            Text("\(line.tool) · \(line.project) · \(line.ticket)")
                .font(.system(size: ConstructTheme.TextSize.caption))
                .foregroundColor(ConstructTheme.color(.mutedForeground))
        }
        .accessibilityElement(children: .combine)
    }
}
