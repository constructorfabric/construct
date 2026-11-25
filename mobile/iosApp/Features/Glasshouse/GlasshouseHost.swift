import ConstructShared
import Foundation

/// What the address bar shows while the person is not typing in it.
enum GlasshouseHost {
    /// The host of [address] with its port, as a browser shows the page it is on: `https://user@example.com:8443/a?b#c`
    /// shows `example.com:8443`. Anything that is not a web address is shown as it is.
    static func display(_ address: String) -> String {
        guard GlasshouseAddress.shared.isWeb(address: address) else { return address }
        let afterScheme = address.trimmingCharacters(in: .whitespacesAndNewlines)
            .drop { $0 != ":" }
            .dropFirst()
            .drop { $0 == "/" || $0 == "\\" }
        let authority = afterScheme.prefix { !"/\\?#".contains($0) }
        return String(authority.split(separator: "@", omittingEmptySubsequences: false).last ?? authority)
    }
}
