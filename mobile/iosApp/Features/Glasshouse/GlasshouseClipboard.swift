import UIKit

/// The system pasteboard, as far as `GlasshouseClipboard` uses it.
protocol GlasshousePasteboard: AnyObject {
    /// What the pasteboard holds, one dictionary of type and data for each item: none when it is empty, and nil when
    /// it holds something this app is not let read. The system lets an app read only what the app put there itself,
    /// or what the person allowed it to read.
    func read() -> [[String: Data]]?
    /// Replaces everything on the pasteboard with [items]; none empties it.
    func replace(with items: [[String: Data]])
}

/// The general pasteboard of the phone.
final class SystemPasteboard: GlasshousePasteboard {
    private let pasteboard = UIPasteboard.general

    func read() -> [[String: Data]]? {
        var items: [[String: Data]] = []
        for index in 0 ..< pasteboard.numberOfItems {
            let item = IndexSet(integer: index)
            var values: [String: Data] = [:]
            for type in pasteboard.types(forItemSet: item)?.first ?? [] {
                guard let data = pasteboard.data(forPasteboardType: type, inItemSet: item)?.first else { return nil }
                values[type] = data
            }
            items.append(values)
        }
        return items
    }

    func replace(with items: [[String: Data]]) {
        pasteboard.setItems(items, options: [:])
    }
}

/// Keeps what the Glasshouse page copies from leaving it. The page's own commands copy to the system pasteboard, and
/// the person may paste that text inside the page. It must not be there for another app, another window of the app or
/// another page of the app, so when the page is left, what the pasteboard held when the page was shown is put back.
///
/// While the page is shown, the only thing that puts something on the pasteboard is the page. A change of the
/// pasteboard while the page is shown is the page's copy, unless what is on the pasteboard then is not the app's to
/// read: that is something another app or another device put there, and it is left alone. What the pasteboard held
/// before is read when the page is shown; if that was something the app is not let read, there is nothing to put back,
/// and the page's text is only taken away.
@MainActor
final class GlasshouseClipboard {
    private let pasteboard: GlasshousePasteboard
    private var shown = false
    private var before: [[String: Data]] = []
    private var changed = false

    init(pasteboard: GlasshousePasteboard = SystemPasteboard()) {
        self.pasteboard = pasteboard
    }

    /// The page is on the screen and the app is in front.
    func pageIsShown() {
        guard !shown else { return }
        shown = true
        changed = false
        before = pasteboard.read() ?? []
    }

    /// Something was put on the pasteboard.
    func pasteboardChanged() {
        if shown { changed = true }
    }

    /// The page is gone from the screen, or the app is leaving the front.
    func pageIsLeft() {
        guard shown else { return }
        shown = false
        guard changed else { return }
        changed = false
        guard pasteboard.read() != nil else { return }
        pasteboard.replace(with: before)
    }
}
