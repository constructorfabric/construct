import ConstructShared
import Foundation

/// How a load the web view could not finish becomes one of the problems the page words.
enum GlasshouseProblems {
    /// The problem for [error], or nil when nothing is wrong: the load was replaced by another one, or stopped
    /// by the app itself. The codes that need the network say `offline` when the phone has none. A certificate
    /// the system does not trust is `certificate`; this is only the word for it, the app never accepts one.
    static func problem(for error: NSError, offline: Bool) -> GlasshouseProblem? {
        guard error.domain == NSURLErrorDomain else {
            // WebKit's "frame load interrupted" is a load the app cancelled by its own decision.
            return error.domain == "WebKitErrorDomain" && error.code == 102 ? nil : .other
        }
        switch URLError.Code(rawValue: error.code) {
        case .cancelled:
            return nil
        case .notConnectedToInternet, .dataNotAllowed, .internationalRoamingOff, .callIsActive:
            return .offline
        case .cannotFindHost, .dnsLookupFailed:
            return offline ? .offline : .notFound
        case .cannotConnectToHost, .networkConnectionLost:
            return offline ? .offline : .refused
        case .timedOut:
            return offline ? .offline : .other
        case .serverCertificateUntrusted, .serverCertificateHasBadDate,
             .serverCertificateHasUnknownRoot, .serverCertificateNotYetValid:
            return .certificate
        default:
            return .other
        }
    }
}
