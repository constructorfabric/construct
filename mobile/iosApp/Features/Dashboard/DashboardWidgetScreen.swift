import ConstructShared
import SwiftUI

/// One widget of the document in a card: its title, what it counts and for which days, and the numbers of the copy as
/// a total, bars per day, a table or the summary of a day. A day or week the computer sent nothing for says so, and
/// shows no zeros.
struct DashboardWidgetCard: View {
    let shown: WidgetState
    let state: DashboardState
    let calendar: DashboardCalendar
    let actions: DashboardActions

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.medium) {
            VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
                Text(DashboardText.title(shown.widget.kind, groupBy: shown.widget.groupBy))
                    .font(.system(size: ConstructTheme.TextSize.body, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
                    .accessibilityAddTraits(.isHeader)
                Text("\(DashboardText.sourceName(shown.source)) · \(period)")
                    .font(.system(size: ConstructTheme.TextSize.caption))
                    .foregroundColor(ConstructTheme.color(.mutedForeground))
            }
            if let numbers = shown.numbers {
                DashboardNumbersView(shown: shown, numbers: numbers, calendar: calendar, actions: actions)
                if numbers.hiddenProjectCount > 0 {
                    Text(DashboardText.hiddenProjects(numbers.hiddenProjectCount))
                        .font(.system(size: ConstructTheme.TextSize.caption))
                        .foregroundColor(ConstructTheme.color(.mutedForeground))
                        .accessibilityIdentifier("dashboard-hidden-\(shown.widget.id)")
                }
            } else {
                Text(DashboardText.notSent(shown.span))
                    .font(.system(size: ConstructTheme.TextSize.secondary))
                    .foregroundColor(ConstructTheme.color(.mutedForeground))
                    .accessibilityIdentifier("dashboard-not-sent-\(shown.widget.id)")
            }
        }
        .padding(ConstructTheme.Space.large)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ConstructTheme.color(.card))
        .clipShape(RoundedRectangle(cornerRadius: ConstructTheme.Radius.medium))
        .overlay(
            RoundedRectangle(cornerRadius: ConstructTheme.Radius.medium)
                .stroke(ConstructTheme.color(.border), lineWidth: ConstructTheme.ControlSize.stroke)
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("dashboard-widget-\(shown.widget.id)")
    }

    /// The day, or the first and the last day, the numbers are for.
    private var period: String {
        guard let day = state.day, let first = state.first, let last = state.last else { return "" }
        return shown.span == .week ? calendar.range(first, last) : calendar.text(day)
    }
}

/// The numbers of a widget, drawn as its kind asks.
private struct DashboardNumbersView: View {
    let shown: WidgetState
    let numbers: DashboardNumbers
    let calendar: DashboardCalendar
    let actions: DashboardActions

    private var id: String { shown.widget.id }

    var body: some View {
        switch shown.widget.kind {
        case .total: total
        case .barsPerDay: bars
        case .table: table
        case .daySummary: summary
        }
    }

    private var total: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
            Text(DashboardText.minutes(numbers.totalMinutes))
                .font(.system(size: ConstructTheme.TextSize.display, weight: .semibold))
                .foregroundColor(ConstructTheme.color(.cardForeground))
                .accessibilityIdentifier("dashboard-value-\(id)")
            Text(DashboardText.sessions(numbers.sessionCount))
                .font(.system(size: ConstructTheme.TextSize.secondary))
                .foregroundColor(ConstructTheme.color(.mutedForeground))
                .accessibilityIdentifier("dashboard-sessions-\(id)")
        }
    }

    private var bars: some View {
        let tallest = numbers.days.map(\.minutes).max() ?? 0
        return HStack(alignment: .bottom, spacing: ConstructTheme.Space.small) {
            ForEach(numbers.days, id: \.label) { day in
                VStack(spacing: ConstructTheme.Space.extraSmall) {
                    ZStack(alignment: .bottom) {
                        Color.clear.frame(height: ConstructTheme.Chart.height)
                        RoundedRectangle(cornerRadius: ConstructTheme.Radius.small)
                            .fill(ConstructTheme.color(day.minutes > 0 ? .chart1 : .muted))
                            .frame(height: barHeight(day.minutes, tallest: tallest))
                    }
                    Text(calendar.weekday(label: day.label))
                        .font(.system(size: ConstructTheme.TextSize.caption))
                        .foregroundColor(ConstructTheme.color(.mutedForeground))
                }
                .frame(maxWidth: ConstructTheme.Chart.barWidth)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(calendar.spoken(label: day.label)), \(DashboardText.minutes(day.minutes))")
                .accessibilityIdentifier("dashboard-bar-\(id)-\(day.label)")
            }
        }
        .frame(maxWidth: .infinity)
    }

    private func barHeight(_ minutes: Int32, tallest: Int32) -> CGFloat {
        guard minutes > 0, tallest > 0 else { return ConstructTheme.Chart.baseline }
        return max(ConstructTheme.Chart.baseline, ConstructTheme.Chart.height * CGFloat(minutes) / CGFloat(tallest))
    }

    @ViewBuilder private var table: some View {
        if numbers.groups.isEmpty {
            empty(DashboardText.noTable)
        } else {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.medium) {
                ForEach(byTime, id: \.key) { group in
                    NavigationLink {
                        DashboardSessionsView(
                            span: shown.span,
                            row: group,
                            sessions: actions.sessionsOf(shown, group)
                        )
                    } label: {
                        GroupRow(group: group, id: id)
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Shows the sessions")
                }
            }
        }
    }

    /// The summary of a day: what the day came to, then one line for each project.
    @ViewBuilder private var summary: some View {
        if numbers.groups.isEmpty {
            empty(DashboardText.noSummary)
        } else {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.small) {
                Text(DashboardText.minutes(numbers.totalMinutes))
                    .font(.system(size: ConstructTheme.TextSize.section, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
                    .accessibilityIdentifier("dashboard-value-\(id)")
                ForEach(byTime, id: \.key) { group in
                    HStack(alignment: .firstTextBaseline) {
                        Text(group.key)
                        Spacer(minLength: ConstructTheme.Space.medium)
                        Text(DashboardText.minutes(group.minutes))
                    }
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("dashboard-line-\(id)-\(group.key)")
                }
            }
        }
    }

    private var byTime: [GroupMinutes] {
        numbers.groups.sorted { $0.minutes > $1.minutes }
    }

    private func empty(_ text: String) -> some View {
        Text(text)
            .font(.system(size: ConstructTheme.TextSize.secondary))
            .foregroundColor(ConstructTheme.color(.mutedForeground))
    }
}

/// A group of a table: its name and time, its share of the whole as a bar and a percent, and its sessions.
private struct GroupRow: View {
    let group: GroupMinutes
    let id: String

    private var percent: Int { Int((group.share * 100).rounded()) }

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
            HStack(alignment: .firstTextBaseline) {
                Text(group.key)
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
                Spacer(minLength: ConstructTheme.Space.medium)
                Text(DashboardText.minutes(group.minutes))
                    .font(.system(size: ConstructTheme.TextSize.body, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.cardForeground))
            }
            HStack(spacing: ConstructTheme.Space.small) {
                share
                Text("\(percent)% · \(DashboardText.sessions(group.sessions))")
                    .font(.system(size: ConstructTheme.TextSize.caption))
                    .foregroundColor(ConstructTheme.color(.mutedForeground))
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("dashboard-row-\(id)-\(group.key)")
    }

    private var share: some View {
        ZStack(alignment: .leading) {
            Capsule().fill(ConstructTheme.color(.muted))
            Capsule()
                .fill(ConstructTheme.color(.chart1))
                .frame(width: ConstructTheme.Chart.shareWidth * CGFloat(min(max(group.share, 0), 1)))
        }
        .frame(width: ConstructTheme.Chart.shareWidth, height: ConstructTheme.Chart.share)
        .accessibilityHidden(true)
    }
}
