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
    }
}

/// Owns the holder and the scope its reads run in, and closes the scope with the page.
@MainActor
final class MemoryModel: ObservableObject {
    let state: FlowState<MemoryState>
    private let scope = UiScope()

    init(construct: Construct) {
        state = FlowState(MemoryHolder(construct: construct, scope: scope).state)
    }

    deinit {
        scope.close()
    }
}
