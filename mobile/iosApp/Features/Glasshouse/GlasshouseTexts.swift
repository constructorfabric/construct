import ConstructShared

/// What the Glasshouse page says, in the words of the desktop app's Glasshouse page.
enum GlasshouseTexts {
    static let reload = "Reload"
    static let notWeb = "Only http and https addresses open here."
    static let noStartAddress = "No start address is set. Type an address above to open a page."

    static let restricted = "Restricted mode"
    static let screenshotOpen = "A screenshot of this page cannot be blocked on this iPhone."
    static let understood = "OK"
    static let pageHidden = "The page is hidden while the screen is recorded or shown on another screen."

    static func blocked(_ action: GlasshouseBlockedAction) -> String {
        switch action {
        case .download: "Download is blocked in restricted mode."
        case .print: "Print is blocked in restricted mode."
        case .share: "Share is blocked in restricted mode."
        }
    }

    static func problem(_ problem: GlasshouseProblem) -> String {
        switch problem {
        case .notFound: "The page cannot open: no site was found at this address."
        case .refused: "The page cannot open: the site refused the connection."
        case .offline: "The page cannot open: there is no network connection."
        case .certificate: "The page cannot open: the site's certificate is not trusted."
        case .crashed: "The page stopped working."
        case .other: "The page cannot open."
        }
    }
}
