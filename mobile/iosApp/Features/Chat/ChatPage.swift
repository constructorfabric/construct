import Combine
import ConstructShared
import SwiftUI

struct ChatPage: View {
    @StateObject private var model: ChatPageModel
    let active: Bool
    let openSettings: () -> Void

    init(
        construct: Construct,
        settings: ModelSettingsStore,
        settingsChanges: AnyPublisher<Void, Never>,
        active: Bool,
        openSettings: @escaping () -> Void
    ) {
        _model = StateObject(wrappedValue: ChatPageModel(
            construct: construct, settings: settings, settingsChanges: settingsChanges
        ))
        self.active = active
        self.openSettings = openSettings
    }

    var body: some View {
        ChatNavigation(
            state: model.state,
            actions: model.actions,
            active: active,
            openSettings: openSettings
        )
    }
}

private struct ChatNavigation: View {
    @ObservedObject var state: FlowState<ChatState>
    let actions: ChatActions
    let active: Bool
    let openSettings: () -> Void

    var body: some View {
        ChatListScreen(state: state.value, actions: actions)
            .navigationDestination(isPresented: Binding(
                get: { state.value.threadOpen },
                set: { if !$0 { actions.close() } }
            )) {
                if let thread = state.value.thread {
                    ChatThreadScreen(state: state.value, thread: thread, actions: actions, openSettings: openSettings)
                        .navigationTitle(thread.title.isEmpty ? "New chat" : thread.title)
                        .navigationBarTitleDisplayMode(.inline)
                        .toolbar(.hidden, for: .tabBar)
                }
            }
            .onChange(of: active) { if $0 { actions.refresh() } }
    }
}

@MainActor
private final class ChatPageModel: ObservableObject {
    let state: FlowState<ChatState>
    private let holder: ChatHolder
    private let scope = UiScope()
    private var settingsObserver: AnyCancellable?

    init(construct: Construct, settings: ModelSettingsStore, settingsChanges: AnyPublisher<Void, Never>) {
        holder = ChatHolder(
            construct: construct,
            settings: settings,
            scope: scope,
            storageDispatcher: BackgroundKt.backgroundDispatcher()
        )
        // The shared holder folds stream events into this state, including text and tool lines.
        state = FlowState(holder.state)
        settingsObserver = settingsChanges.sink { [holder] in holder.refreshModel() }
    }

    var actions: ChatActions {
        ChatActions(
            newChat: { [holder] in holder.doNewChat() },
            open: { [holder] in holder.open(summary: $0) },
            close: { [holder] in holder.closeThread() },
            refresh: { [holder] in holder.refresh() },
            draft: { [holder] in holder.state.value.draft },
            edit: { [holder] in holder.changeDraft(text: $0) },
            send: { [holder] in holder.send(text: $0) },
            stop: { [holder] in holder.stop() },
            retry: { [holder] in holder.retry() },
            delete: { [holder] in holder.delete(which: $0) }
        )
    }

    deinit {
        scope.close()
    }
}
