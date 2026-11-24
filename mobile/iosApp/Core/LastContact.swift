import Foundation

/// The time the computer last answered, as the connection line and the Computer page show it.
enum LastContact {
    /// The saved time, or nil when the computer has not answered yet.
    static func date(millis: Int64) -> Date? {
        millis > 0 ? Date(timeIntervalSince1970: TimeInterval(millis) / 1000) : nil
    }

    static func text(_ date: Date) -> String {
        "Last contact: \(date.formatted(date: .abbreviated, time: .shortened))"
    }
}
