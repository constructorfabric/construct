import ConstructShared
import SwiftUI

/// Builds the holder of the Memory page and shows `MemoryScreen` with its state.
struct MemoryPage: View {
    @StateObject private var model: MemoryModel

    init(construct: Construct) {
        _model = StateObject(wrappedValue: MemoryModel(construct: construct))
    }

    var body: some View {
        MemoryScreen(state: model.state)
            .onAppear(perform: model.refresh)
    }
}

/// Owns the holder and the scope its reads run in, and closes the scope with the page.
@MainActor
final class MemoryModel: ObservableObject {
    let state: FlowState<MemoryState>
    private let holder: MemoryHolder
    private let scope = UiScope()

    init(construct: Construct) {
        holder = MemoryHolder(construct: construct, scope: scope)
        state = FlowState(holder.state)
    }

    func refresh() {
        holder.refresh()
    }

    deinit {
        scope.close()
    }
}
