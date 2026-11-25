import ConstructShared
import Foundation

/// What the page does with a navigation the web view is about to make: a link, a redirect, a script, a new window.
enum GlasshouseNavigation {
    /// Where the navigation goes.
    enum Frame {
        case main
        /// A frame inside the page.
        case inner
        /// A window the page asked for.
        case newWindow
    }

    enum Decision {
        case allow
        case cancel
        /// A web address in a new window opens in this page instead.
        case openHere
    }

    /// Only web addresses open, whoever asks: a link, a redirect or a new window to any other kind of address does
    /// nothing, and no other app starts. A frame inside the page may be empty (`about:blank`).
    static func decide(_ url: URL, frame: Frame) -> Decision {
        if GlasshouseAddress.shared.isWeb(address: url.absoluteString) {
            return frame == .newWindow ? .openHere : .allow
        }
        return frame == .inner && url.scheme == "about" ? .allow : .cancel
    }
}
