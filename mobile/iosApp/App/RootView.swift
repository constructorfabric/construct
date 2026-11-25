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
    @StateObject private var glasshouse: GlasshouseModel

    init(composition: Composition) {
        self.composition = composition
        _connection = StateObject(wrappedValue: ConnectionModel(connection: composition.phoneConnection))
        // The shell keeps the Glasshouse page, so its web view is not made again when another page is shown.
        _glasshouse = StateObject(wrappedValue: GlasshouseModel(
            startAddress: GlasshouseStartAddress.configured(),
            version: GlasshouseStartAddress.appVersion()
        ))
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
            case .glasshouse: GlasshousePage(model: glasshouse)
            case .settings:
                let status = connection.state.value.computerStatus
                SettingsPage(
                    settings: composition.modelSettings,
                    status: status,
                    onCommit: { composition.modelSettingsDidCommit.send(()) },
                    computer: {
                        ComputerPage(
                            status: connection.state.$value.map(\.computerStatus).eraseToAnyPublisher(),
                            connection: composition.phoneConnection,
                            copy: composition.copy,
                            activity: ComputerActivity.init(_:),
                            retry: connection.holder.tryAgain
                        )
                    }
                )
            default: PlaceholderPage(page: page)
            }
        }
        .onAppear { connection.holder.setForeground(active: scenePhase == .active) }
        .onChange(of: scenePhase) { phase in connection.holder.setForeground(active: phase == .active) }
    }
}

extension ComputerActivity {
    /// The screen's reading of a pairing result, which `feature/status` words for both phones.
    init(_ result: PairResult) {
        let operation = result.toOperation()
        self.init(
            busy: operation.busy,
            paired: operation.paired,
            message: operation.message,
            needsNetworkPermission: operation.needsNetworkPermission,
            needsFreshCode: operation.needsFreshCode
        )
    }
}

private extension ConnectionStatus {
    var computerStatus: ComputerStatus {
        let unreachable = kind == .unreachable || kind == .noNetwork
        let needsLocalNetwork = kind == .networkPermission
        return ComputerStatus(
            label: unreachable ? "Not reachable" : kind.title,
            notice: needsLocalNetwork
                ? ComputerStatus.allowLocalNetwork
                : kind.title + (kind.detail.map { ". \($0)" } ?? "."),
            endpoint: endpoint,
            lastContact: LastContact.date(millis: lastContactMillis),
            checking: kind == .checking,
            unpaired: kind == .notPaired,
            connected: kind == .connected,
            canRetry: kind.canRetry,
            needsLocalNetwork: needsLocalNetwork,
            dot: kind.dot
        )
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
