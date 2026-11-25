import Combine
import ConstructShared
import SwiftUI

/// The shell: the frame and the page each tab shows. A page is a folder under `Features/`.
struct RootView: View {
    let composition: Composition
    @Environment(\.scenePhase) private var scenePhase
    @State private var selected = Page.chat
    @StateObject private var connection: ConnectionModel
    @StateObject private var sync: SyncModel
    @StateObject private var glasshouse: GlasshouseModel

    init(composition: Composition) {
        self.composition = composition
        _connection = StateObject(wrappedValue: ConnectionModel(connection: composition.phoneConnection))
        _sync = StateObject(wrappedValue: SyncModel(syncer: composition.syncer, timing: composition.syncTiming))
        // The shell keeps the Glasshouse page, so its web view is not made again when another page is shown.
        _glasshouse = StateObject(wrappedValue: GlasshouseModel(
            startAddress: GlasshouseStartAddress.configured(),
            version: GlasshouseStartAddress.appVersion()
        ))
        ConstructTheme.styleBars()
    }

    var body: some View {
        let copying = firstCopy
        frame
            .accessibilityHidden(copying != nil)
            .overlay {
                // On top of the tabs, not instead of them: the page the person was on is still there when the copy
                // is done.
                if let copying {
                    FirstCopyScreen(state: copying)
                        .accessibilityAddTraits(.isModal)
                }
            }
            .onAppear { setForeground(scenePhase == .active) }
            .onChange(of: scenePhase) { phase in setForeground(phase == .active) }
            .onChange(of: sync.state.value.notice) { SyncAnnouncement.post($0) }
    }

    /// The first copy shows only for a phone that is paired and reaches its computer: a phone that is not paired has
    /// no copy to make, and a computer that does not answer is told by the notice, with the pages and the Computer
    /// page still open so the person can try again, pair again or unpair.
    private var firstCopy: FirstCopy? {
        guard connection.state.value.kind == .connected else { return nil }
        return sync.state.value.firstCopy.flatMap { $0.running ? $0 : nil }
    }

    private func setForeground(_ active: Bool) {
        connection.holder.setForeground(active: active)
        sync.holder.setForeground(active: active)
    }

    /// Checks the computer again and copies from it: "Try again" on a notice and on the Computer page.
    private var tryAgain: () -> Void {
        { [connection, sync] in
            connection.holder.tryAgain()
            sync.holder.syncNow()
        }
    }

    private var frame: some View {
        AppFrame(
            syncNotice: SyncNoticeContent(
                notice: sync.state.value.notice,
                lastSynced: SyncText.date(sync.state.value.lastSyncedAt),
                tryAgain: tryAgain
            ),
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
            case .dashboard:
                DashboardPage(
                    construct: composition.construct,
                    copy: composition.copy,
                    lastSynced: sync.state.value.lastSyncedAt
                )
            case .glasshouse: GlasshousePage(model: glasshouse)
            case .settings:
                SettingsPage(
                    settings: composition.modelSettings,
                    status: computerStatus(connection.state.value, sync.state.value),
                    onCommit: { composition.modelSettingsDidCommit.send(()) },
                    computer: {
                        ComputerPage(
                            status: Publishers.CombineLatest(connection.state.$value, sync.state.$value)
                                .map { computerStatus($0, $1) }
                                .eraseToAnyPublisher(),
                            connection: composition.phoneConnection,
                            copy: composition.copy,
                            activity: ComputerActivity.init(_:),
                            retry: tryAgain,
                            syncNow: sync.holder.syncNow,
                            afterPairing: sync.holder.syncNow
                        )
                    }
                )
            }
        }
    }
}

/// What the Computer page and the Settings row say about the connection and the copy.
private func computerStatus(_ connection: ConnectionStatus, _ sync: SyncState) -> ComputerStatus {
    let kind = connection.kind
    let unreachable = kind == .unreachable || kind == .noNetwork
    let needsLocalNetwork = kind == .networkPermission
    return ComputerStatus(
        label: unreachable ? "Not reachable" : kind.title,
        notice: needsLocalNetwork
            ? ComputerStatus.allowLocalNetwork
            : kind.title + (kind.detail.map { ". \($0)" } ?? "."),
        endpoint: connection.endpoint,
        lastSynced: SyncText.date(sync.lastSyncedAt),
        syncing: sync.notice == .syncing,
        checking: kind == .checking,
        unpaired: kind == .notPaired,
        connected: kind == .connected,
        canRetry: kind.canRetry,
        needsLocalNetwork: needsLocalNetwork,
        dot: kind.dot
    )
}

extension ConnectionStatusKind {
    /// The dot of a state: green when connected, red when the computer cannot be reached, grey while the state is
    /// not known.
    var dot: ColorRole {
        switch self {
        case .connected: return .success
        case .checking, .notPaired: return .borderStrong
        default: return .destructive
        }
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
