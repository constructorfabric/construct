import ConstructShared
import SwiftUI

/// The one adapter between a Kotlin state holder and SwiftUI. It watches the holder's `StateFlow` and
/// publishes each new state, so a view reads `value` and redraws when it changes. Every feature uses it.
///
/// The Kotlin flow is read as a Swift `AsyncSequence`, which the SKIE Gradle plugin generates.
@MainActor
final class FlowState<Value: AnyObject>: ObservableObject {
    @Published private(set) var value: Value
    private var watching: Task<Void, Never>?

    init(_ flow: SkieSwiftStateFlow<Value>) {
        value = flow.value
        watching = Task { [weak self] in
            for await next in flow {
                self?.value = next
            }
        }
    }

    deinit {
        watching?.cancel()
    }
}
