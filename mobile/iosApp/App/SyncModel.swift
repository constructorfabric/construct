import Combine
import ConstructShared
import SwiftUI

/// Owns the sync holder of the app and the scope it runs in, like `ConnectionModel` does for the connection. The shell
/// keeps it, because the frame, the loading screen and the Computer page all show what it knows.
@MainActor
final class SyncModel: ObservableObject {
    let holder: SyncHolder
    let state: FlowState<SyncState>
    private let scope = UiScope()
    private var forwarding: AnyCancellable?

    init(syncer: Syncer, timing: SyncTiming) {
        holder = SyncHolder(
            syncer: syncer,
            scope: scope,
            intervalMillis: timing.interval,
            justSyncedMillis: timing.justSynced
        )
        state = FlowState(holder.state)
        forwarding = state.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }

    deinit {
        scope.close()
    }
}
