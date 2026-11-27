import ConstructShared
import SwiftUI

/// Builds the holder of the Dashboard page and shows `DashboardScreen` with its state. The page reads only the copy on
/// the phone, so it works with the computer off; a sync that changes the copy shows by itself, and one that only
/// brings it up to date (`lastSynced` changes) reads the time again.
struct DashboardPage: View {
    @StateObject private var model: DashboardModel
    private let lastSynced: String?

    init(construct: Construct, copy: CopyStore, lastSynced: String?) {
        _model = StateObject(wrappedValue: DashboardModel(construct: construct, copy: copy))
        self.lastSynced = lastSynced
    }

    var body: some View {
        DashboardScreen(state: model.state, actions: model.actions)
            .onAppear(perform: model.refresh)
            .onChange(of: lastSynced) { _ in model.refresh() }
    }
}

/// What the person can do on the page, so that the screen needs no state holder.
struct DashboardActions {
    let showSpan: (DashboardSpan) -> Void
    let showSource: (Source) -> Void
    let previous: () -> Void
    let next: () -> Void
    let showToday: () -> Void
    let pick: (CalendarDay) -> Void
}

/// Owns the holder and the scope its reads run in, and closes the scope with the page.
@MainActor
final class DashboardModel: ObservableObject {
    let state: FlowState<DashboardState>
    private let holder: DashboardHolder
    private let scope = UiScope()

    init(construct: Construct, copy: CopyStore) {
        holder = DashboardHolder(construct: construct, record: { copy.syncRecord() }, scope: scope)
        state = FlowState(holder.state)
    }

    var actions: DashboardActions {
        DashboardActions(
            showSpan: holder.showSpan,
            showSource: holder.showSource,
            previous: holder.previous,
            next: holder.next,
            showToday: holder.showToday,
            pick: holder.pick
        )
    }

    func refresh() {
        holder.refresh()
    }

    deinit {
        scope.close()
    }
}
