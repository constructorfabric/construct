import Network
import os

enum LocalNetworkAccess {
    private static let queue = DispatchQueue(label: "Construct local network access")

    /// Called on the connection's I/O dispatcher after a failed transport attempt.
    static func isDenied(host: String, port: Int32) -> Bool {
        guard let number = UInt16(exactly: port), let endpointPort = NWEndpoint.Port(rawValue: number) else {
            return false
        }
        let probe = NWConnection(host: NWEndpoint.Host(host), port: endpointPort, using: .tcp)
        let finished = DispatchSemaphore(value: 0)
        let denied = OSAllocatedUnfairLock(initialState: false)
        probe.stateUpdateHandler = { [weak probe] state in
            switch state {
            case .waiting:
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
