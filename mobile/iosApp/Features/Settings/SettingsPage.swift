import ConstructShared
import SwiftUI

struct SettingsPage<Computer: View>: View {
    @StateObject private var model: SettingsModel
    let status: ComputerStatus
    let computer: Computer

    init(
        settings: ModelSettingsStore,
        status: ComputerStatus,
        onCommit: @escaping () -> Void,
        @ViewBuilder computer: () -> Computer
    ) {
        _model = StateObject(wrappedValue: SettingsModel(settings: settings, onCommit: onCommit))
        self.status = status
        self.computer = computer()
    }

    var body: some View {
        SettingsScreen(state: model.state, actions: model.actions, status: status, computer: computer)
    }
}

@MainActor
private final class SettingsModel: ObservableObject {
    let state: FlowState<SettingsState>
    private let holder: SettingsHolder
    private let scope = UiScope()

    init(settings: ModelSettingsStore, onCommit: @escaping () -> Void) {
        holder = SettingsHolder(
            store: settings,
            core: ChatCoreKt.nativeChatCore(),
            scope: scope,
            io: BackgroundKt.backgroundDispatcher(),
            onCommit: onCommit
        )
        state = FlowState(holder.state)
    }

    var actions: SettingsActions {
        SettingsActions(
            edit: { [holder] field, value in holder.change(field: field, value: value) },
            save: { [holder] in holder.save() },
            test: { [holder] in holder.test() },
            remove: { [holder] in holder.removeKey() },
            reload: { [holder] in holder.load() }
        )
    }

    deinit {
        scope.close()
    }
}
