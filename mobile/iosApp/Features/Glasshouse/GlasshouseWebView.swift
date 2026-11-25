import UIKit
import WebKit

/// The web view of the Glasshouse page. The touch menu of a selection offers no "Copy", "Cut" or "Share": the menu asks
/// the web view whether it can perform each action, and this one says no to those three. WebKit's own "Copy Link with
/// Highlight" is not asked about; where UIKit lets an app remove it (iOS 26 and later), it is removed from the menu.
final class GlasshouseWebView: WKWebView {
    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        GlasshouseRestrictions.allows(menuAction: action) && super.canPerformAction(action, withSender: sender)
    }

    override func buildMenu(with builder: UIMenuBuilder) {
        super.buildMenu(with: builder)
        if #available(iOS 26.0, *) {
            builder.remove(action: GlasshouseRestrictions.copyLinkWithHighlight)
        }
    }
}
