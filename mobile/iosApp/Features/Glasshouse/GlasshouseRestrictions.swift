import UIKit
import WebKit

/// An action restricted mode refuses on the Glasshouse page, and says so in one line.
enum GlasshouseBlockedAction: String {
    case download
    case print
    case share
}

/// What restricted mode takes away from the page, apart from the clipboard (`GlasshouseClipboard`) and what a
/// recording shows (`GlasshouseRestriction`).
enum GlasshouseRestrictions {
    /// The script message handler that the page's script tells a refused print or share to.
    static let messageName = "glasshouseBlocked"

    /// The actions of the touch menu that the page does not offer: "Copy", "Cut" and "Share". The menu asks the web
    /// view whether it can perform each action by its selector; sharing has no public one, so the menu's own name for
    /// it is matched too.
    private static let refusedMenuActions: Set<String> = ["copy:", "cut:", "share:", "_share:"]

    /// WebKit's menu action "Copy Link with Highlight", which puts a link that holds the selected text on the
    /// pasteboard. The name is WebKit's own; UIKit can remove an action by it from iOS 26.
    static let copyLinkWithHighlight = UIAction.Identifier("WKActionScrollToTextFragmentGeneration")

    static func allows(menuAction action: Selector) -> Bool {
        !refusedMenuActions.contains(NSStringFromSelector(action))
    }

    /// Whether a response is a file to save, not a page to show: a type the web view cannot show, or a response the
    /// server marks as an attachment. The page never starts a download, so it is cancelled.
    static func isDownload(_ response: URLResponse, canShow: Bool) -> Bool {
        guard canShow else { return true }
        let disposition = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Disposition")
        return disposition?.trimmingCharacters(in: .whitespaces).lowercased().hasPrefix("attachment") ?? false
    }

    /// Print and share are the page's own commands, so the page's script is what refuses them: it tells the app, and
    /// a share's promise is rejected as a person who says no would.
    static let script = """
    (function () {
      var tell = function (action) {
        try { window.webkit.messageHandlers.\(messageName).postMessage(action); } catch (error) {}
      };
      var define = function (target, name, value) {
        try {
          Object.defineProperty(target, name, {value: value, configurable: true, writable: true});
        } catch (error) {}
      };
      define(window, 'print', function () { tell('print'); });
      define(Navigator.prototype, 'share', function () {
        tell('share');
        return Promise.reject(new DOMException('Sharing is blocked in restricted mode.', 'NotAllowedError'));
      });
      define(Navigator.prototype, 'canShare', function () { return false; });
    })();
    """

    /// Runs in every page and every frame of it, before the page's own scripts.
    static func userScript() -> WKUserScript {
        WKUserScript(source: script, injectionTime: .atDocumentStart, forMainFrameOnly: false)
    }

    /// The action a script message names, or nil for anything else.
    static func action(in message: Any) -> GlasshouseBlockedAction? {
        (message as? String).flatMap(GlasshouseBlockedAction.init(rawValue:))
    }
}

/// Hands what the page's script says to a closure. A web view keeps its message handlers for as long as it lives, so
/// the closure must not hold the owner of the web view.
@MainActor
final class GlasshouseBlockedMessages: NSObject, WKScriptMessageHandler {
    private let handler: @MainActor (GlasshouseBlockedAction) -> Void

    init(_ handler: @escaping @MainActor (GlasshouseBlockedAction) -> Void) {
        self.handler = handler
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let action = GlasshouseRestrictions.action(in: message.body) else { return }
        handler(action)
    }
}
