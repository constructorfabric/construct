import ConstructShared
import SwiftUI

/// The Memory page: the person's facts as a plain list. It shows the state of the shared holder and
/// loads nothing itself.
struct MemoryScreen: View {
    @StateObject private var model: MemoryModel

    init(construct: Construct) {
        _model = StateObject(wrappedValue: MemoryModel(construct: construct))
    }

    var body: some View {
        MemoryWatcher(state: model.state)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(ConstructTheme.color(.background))
    }
}

/// Redraws `MemoryContent` each time the adapter publishes a new state.
private struct MemoryWatcher: View {
    @ObservedObject var state: FlowState<MemoryState>

    var body: some View {
        MemoryContent(state: state.value)
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

/// Draws one `MemoryState`.
struct MemoryContent: View {
    let state: MemoryState

    var body: some View {
        if let failure = state.failure {
            Text(failure)
                .foregroundColor(ConstructTheme.color(.destructive))
        } else if state.loading && state.facts.isEmpty {
            ProgressView()
        } else {
            List(state.facts, id: \.factId) { fact in
                FactRow(fact: fact)
                    .listRowBackground(ConstructTheme.color(.surface))
            }
            .listStyle(.plain)
        }
    }
}

private struct FactRow: View {
    let fact: MemoryFact

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
            Text(fact.property)
                .font(.system(size: ConstructTheme.TextSize.secondary))
                .foregroundColor(ConstructTheme.color(.mutedForeground))
            Text(display(fact.value))
                .font(.system(size: ConstructTheme.TextSize.body))
                .foregroundColor(ConstructTheme.color(.foreground))
        }
        .padding(.vertical, ConstructTheme.Space.small)
    }

    /// A string shows as itself, any other value as its JSON text.
    private func display(_ value: Kotlinx_serialization_jsonJsonElement) -> String {
        if let primitive = value as? Kotlinx_serialization_jsonJsonPrimitive, primitive.isString {
            return primitive.content
        }
        return value.description
    }
}
