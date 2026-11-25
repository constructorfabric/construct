import Foundation
import Network
#if DEBUG && targetEnvironment(simulator)
import notify
#endif

/// Whether the phone has no network to ask for a page with. The page asks before it opens or reloads one, so a
/// phone that has lost its network says so at once, and Reload asks again.
///
/// It says "offline" only when the system has said so: until the first answer, and when the system is still
/// working out a connection, a page is asked for.
final class GlasshouseReachability: @unchecked Sendable {
    private let monitor = NWPathMonitor()
    private let lock = NSLock()
    private var unsatisfied = false
    private var forced: Bool?

    init() {
        monitor.pathUpdateHandler = { [weak self] path in self?.set(unsatisfied: path.status == .unsatisfied) }
        monitor.start(queue: DispatchQueue(label: "Construct Glasshouse network"))
        #if DEBUG && targetEnvironment(simulator)
        listenForTests()
        #endif
    }

    deinit {
        monitor.cancel()
    }

    var isOffline: Bool {
        lock.lock()
        defer { lock.unlock() }
        return forced ?? unsatisfied
    }

    private func set(unsatisfied value: Bool) {
        lock.lock()
        unsatisfied = value
        lock.unlock()
    }

    #if DEBUG && targetEnvironment(simulator)
    /// A Simulator has no network of its own to switch off, so the page tests say to this build when the phone
    /// has none, with a notification between the test runner and the app. Only a debug Simulator build listens.
    static let offlineNotification = "construct.glasshouse.test.offline"
    static let onlineNotification = "construct.glasshouse.test.online"
    private var tokens: [Int32] = []

    private func listenForTests() {
        for (name, offline) in [(Self.offlineNotification, true), (Self.onlineNotification, false)] {
            var token: Int32 = 0
            notify_register_dispatch(name, &token, DispatchQueue.global()) { [weak self] _ in
                self?.lock.lock()
                self?.forced = offline
                self?.lock.unlock()
            }
            tokens.append(token)
        }
    }
    #endif
}
