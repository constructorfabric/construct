import ConstructShared
import SwiftUI

/// The Memory screen: the person's facts as a plain list. It shows the state it is given and loads
/// nothing itself.
struct MemoryScreen: View {
    @ObservedObject var state: FlowState<MemoryState>

    var body: some View {
        MemoryContent(state: state.value)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(ConstructTheme.color(.background))
    }
}

/// Draws one `MemoryState`.
struct MemoryContent: View {
    let state: MemoryState

    var body: some View {
        if let failure = state.failure {
            SyncNoticePage {
                Text(failure)
                    .foregroundColor(ConstructTheme.color(.destructive))
            }
        } else if state.loading && state.facts.isEmpty {
            SyncNoticePage { ProgressView() }
        } else {
            List {
                SyncNoticeRow()
                ForEach(state.facts, id: \.factId) { fact in
                    FactRow(fact: fact)
                        .listRowBackground(ConstructTheme.color(.surface))
                }
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
