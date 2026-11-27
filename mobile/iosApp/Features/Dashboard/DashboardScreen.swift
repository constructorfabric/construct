import ConstructShared
import SwiftUI

/// The Dashboard screen: the computer's dashboard as the copy on the phone holds it, in one column. It shows the
/// state it is given and loads nothing itself.
struct DashboardScreen: View {
    @ObservedObject var state: FlowState<DashboardState>
    let actions: DashboardActions

    var body: some View {
        DashboardContent(state: state.value, actions: actions)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(ConstructTheme.color(.background))
    }
}

/// Draws one `DashboardState`.
struct DashboardContent: View {
    let state: DashboardState
    let actions: DashboardActions

    var body: some View {
        if state.loading && !state.hasDashboard && state.failure == nil {
            SyncNoticePage { ProgressView() }
        } else if state.isEmpty {
            SyncNoticePage { DashboardEmpty() }
        } else {
            page
        }
    }

    private var page: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.large) {
                SyncNoticeRow()
                VStack(alignment: .leading, spacing: ConstructTheme.Space.large) {
                    if let failure = state.failure {
                        Text(failure)
                            .font(.system(size: ConstructTheme.TextSize.secondary))
                            .foregroundColor(ConstructTheme.color(.destructive))
                    }
                    if state.hasDashboard {
                        DashboardHeader(state: state, actions: actions)
                        widgets
                    }
                }
                .padding(.horizontal, ConstructTheme.Space.large)
                .padding(.bottom, ConstructTheme.Space.large)
            }
        }
    }

    @ViewBuilder private var widgets: some View {
        let calendar = DashboardCalendar(zone: state.zoneName)
        if state.nothingSent {
            Text(DashboardText.notSent(state.span))
                .font(.system(size: ConstructTheme.TextSize.body))
                .foregroundColor(ConstructTheme.color(.mutedForeground))
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("dashboard-not-sent")
        } else {
            ForEach(state.widgets, id: \.widget.id) { shown in
                DashboardWidgetCard(shown: shown, state: state, calendar: calendar, actions: actions)
            }
        }
    }
}

/// What the page says when the copy holds no dashboard: before the first copy, and after "Delete everything" on the
/// computer.
private struct DashboardEmpty: View {
    var body: some View {
        VStack(spacing: ConstructTheme.Space.medium) {
            Image(Page.dashboard.icon)
                .resizable()
                .scaledToFit()
                .frame(width: ConstructTheme.ControlSize.iconLarge, height: ConstructTheme.ControlSize.iconLarge)
            Text(DashboardText.emptyTitle)
                .font(.system(size: ConstructTheme.TextSize.body, weight: .semibold))
            Text(DashboardText.emptyText)
                .font(.system(size: ConstructTheme.TextSize.secondary))
                .multilineTextAlignment(.center)
        }
        .foregroundColor(ConstructTheme.color(.mutedForeground))
        .padding(.horizontal, ConstructTheme.Space.extraLarge)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("dashboard-empty")
    }
}

/// The title with the time of the last update, and the controls that move the page: day or week, back, forward, a date.
private struct DashboardHeader: View {
    let state: DashboardState
    let actions: DashboardActions
    @State private var picking = false

    private var calendar: DashboardCalendar { DashboardCalendar(zone: state.zoneName) }
    private var weeks: Bool { state.span == .week }

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.medium) {
            VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
                Text(state.title)
                    .font(.system(size: ConstructTheme.TextSize.heading, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.foreground))
                    .accessibilityAddTraits(.isHeader)
                Text(DashboardText.updated(state.updatedAt))
                    .font(.system(size: ConstructTheme.TextSize.secondary))
                    .foregroundColor(ConstructTheme.color(.mutedForeground))
                    .accessibilityIdentifier("dashboard-updated")
            }
            Picker("Show", selection: spanBinding) {
                Text("Day").tag(DashboardSpan.day)
                Text("Week").tag(DashboardSpan.week)
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier("dashboard-span")
            HStack(spacing: ConstructTheme.Space.small) {
                step("chevron.left", label: weeks ? "Previous week" : "Previous day", id: "dashboard-previous") {
                    actions.previous()
                }
                Button { picking = true } label: {
                    Text(rangeText)
                        .font(.system(size: ConstructTheme.TextSize.body, weight: .semibold))
                        .foregroundColor(ConstructTheme.color(.linkForeground))
                        .frame(maxWidth: .infinity, minHeight: ConstructTheme.ControlSize.touch)
                }
                .accessibilityLabel(rangeText)
                .accessibilityHint("Picks a date")
                .accessibilityIdentifier("dashboard-date")
                step("chevron.right", label: weeks ? "Next week" : "Next day", id: "dashboard-next",
                     enabled: state.canGoNext) {
                    actions.next()
                }
            }
            if !state.onToday {
                Button("Today", action: actions.showToday)
                    .font(.system(size: ConstructTheme.TextSize.secondary, weight: .semibold))
                    .foregroundColor(ConstructTheme.color(.linkForeground))
                    .frame(minHeight: ConstructTheme.ControlSize.touch)
                    .accessibilityIdentifier("dashboard-today")
            }
        }
        .sheet(isPresented: $picking) { picker }
    }

    private var spanBinding: Binding<DashboardSpan> {
        Binding(get: { state.span }, set: { actions.showSpan($0) })
    }

    private var rangeText: String {
        guard let first = state.first, let last = state.last else { return "" }
        return calendar.range(first, last)
    }

    /// A step of the page. One that cannot be taken is dimmed, so the person sees that there is no later day.
    private func step(_ icon: String, label: String, id: String, enabled: Bool = true,
                      action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .frame(width: ConstructTheme.ControlSize.touch, height: ConstructTheme.ControlSize.touch)
        }
        .disabled(!enabled)
        .foregroundStyle(enabled ? ConstructTheme.color(.linkForeground) : ConstructTheme.color(.mutedForeground))
        .opacity(enabled ? 1 : ConstructTheme.Loading.dimmed)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
    }

    /// The calendar of the computer's days, up to today: nothing is counted for the future.
    private var picker: some View {
        NavigationStack {
            DatePicker("Date", selection: dateBinding, in: ...(today ?? Date.distantFuture), displayedComponents: .date)
                .datePickerStyle(.graphical)
                .environment(\.calendar, calendar.calendar)
                .environment(\.timeZone, calendar.calendar.timeZone)
                .padding(.horizontal, ConstructTheme.Space.large)
                .accessibilityIdentifier("dashboard-calendar")
                .navigationTitle("Pick a date")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { picking = false }
                            .accessibilityIdentifier("dashboard-calendar-done")
                    }
                }
        }
        .presentationDetents([.medium])
    }

    private var today: Date? { state.today.map(calendar.date) }

    private var dateBinding: Binding<Date> {
        Binding(
            get: { state.day.map(calendar.date) ?? Date() },
            set: { actions.pick(calendar.day(of: $0)) }
        )
    }
}
