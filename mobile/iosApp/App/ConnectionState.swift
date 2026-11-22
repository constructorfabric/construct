/// How the app stands to the person's Construct. `current()` says which, per build type.
enum ConnectionState {
    case connected
    case notConnected

    var text: String {
        switch self {
        case .connected: "Connected"
        case .notConnected: "Not connected"
        }
    }

    /// Debug builds read the sample data, which is always there. Release builds have no pairing yet,
    /// so there is nothing to be connected to.
    static func current() -> ConnectionState {
        #if DEBUG
        .connected
        #else
        .notConnected
        #endif
    }
}
