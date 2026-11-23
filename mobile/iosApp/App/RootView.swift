import Combine
import ConstructShared
import SwiftUI

/// The shell: the frame and the page each tab shows. A page with a feature is a folder under
/// `Features/`; one without is a `PlaceholderPage` until its feature takes it over.
struct RootView: View {
    let composition: Composition
    @Environment(\.scenePhase) private var scenePhase
    @State private var selected = Page.chat
    @StateObject private var connection: ConnectionModel

    init(composition: Composition) {
        self.composition = composition
        _connection = StateObject(wrappedValue: ConnectionModel(connection: composition.phoneConnection))
        ConstructTheme.styleBars()
    }

    var body: some View {
        AppFrame(
            connection: connection.state.value,
            onTryAgain: connection.holder.tryAgain,
            selected: $selected
        ) { page in
            switch page {
            case .chat:
                ChatPage(
                    construct: composition.construct,
                    settings: composition.modelSettings,
                    settingsChanges: composition.modelSettingsDidCommit.eraseToAnyPublisher(),
                    active: selected == .chat,
                    openSettings: { selected = .settings }
                )
            case .memory: MemoryPage(construct: composition.construct)
            case .settings:
                SettingsPage(settings: composition.modelSettings) {
                    composition.modelSettingsDidCommit.send(())
                }
            default: PlaceholderPage(page: page)
            }
        }
        .onAppear { connection.holder.setForeground(active: scenePhase == .active) }
        .onChange(of: scenePhase) { phase in connection.holder.setForeground(active: phase == .active) }
    }
}

@MainActor
final class ConnectionModel: ObservableObject {
    let holder: ConnectionHolder
    let state: FlowState<ConnectionStatus>
    private let scope = UiScope()
    private var forwarding: AnyCancellable?

    init(connection: PhoneConnection) {
        holder = ConnectionHolder(connection: connection, scope: scope)
        state = FlowState(holder.state)
        forwarding = state.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }

    deinit {
        scope.close()
    }
}
