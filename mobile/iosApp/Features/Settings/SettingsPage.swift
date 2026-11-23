import ConstructShared
import SwiftUI

struct SettingsPage: View {
    @StateObject private var model: SettingsModel

    init(settings: ModelSettingsStore, onCommit: @escaping () -> Void) {
        _model = StateObject(wrappedValue: SettingsModel(settings: settings, onCommit: onCommit))
    }

    var body: some View {
        SettingsScreen(state: model.state, actions: model.actions)
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
