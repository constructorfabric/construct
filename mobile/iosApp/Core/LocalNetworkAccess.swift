import Network
import os

/// Tells a refused Local Network permission from a computer that does not answer.
///
/// An iPhone cannot be asked for this permission ahead of use, and the app cannot read the answer. The
/// system asks the first time the app opens a connection on the local network, and a refusal shows up
/// only as a connection that does not open. So after a connection has failed, the app opens a second
/// one to the same address and asks the system why it waits.
enum LocalNetworkAccess {
    private static let queue = DispatchQueue(label: "Construct local network access")

    /// Whether the system holds back a connection to this address because the permission is refused.
    ///
    /// It blocks the caller for up to a second, so the caller must be off the main thread: the connection
    /// calls it on its own I/O dispatcher, after a failed attempt. If the system has not answered in that
    /// second, the answer is "no": the person then sees that the computer is not reachable, and the next
    /// check or Try again asks again.
    static func isDenied(host: String, port: Int32) -> Bool {
        guard let number = UInt16(exactly: port), let endpointPort = NWEndpoint.Port(rawValue: number) else {
            return false
        }
        let probe = NWConnection(host: NWEndpoint.Host(host), port: endpointPort, using: .tcp)
        // The state handler runs on `queue`, and the caller waits here for its first final state.
        let finished = DispatchSemaphore(value: 0)
        let denied = OSAllocatedUnfairLock(initialState: false)
        probe.stateUpdateHandler = { [weak probe] state in
            switch state {
            case .waiting:
                // A connection that cannot start waits. Its path says why: `.localNetworkDenied` is the
                // refused permission, and the only sign of it the system gives.
                let permissionDenied = probe?.currentPath?.unsatisfiedReason == .localNetworkDenied
                denied.withLock { $0 = permissionDenied }
            case .ready, .failed, .cancelled:
                break
            default:
                return
            }
            finished.signal()
        }
        probe.start(queue: queue)
        defer { probe.cancel() }
        guard finished.wait(timeout: .now() + .seconds(1)) == .success else { return false }
        return denied.withLock { $0 }
    }
}
