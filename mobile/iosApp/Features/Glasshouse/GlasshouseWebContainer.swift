import SwiftUI
import WebKit

/// Puts the page's one web view on the screen, in restricted mode: kept out of screenshots and recordings while it
/// is on the screen. The web view is made and kept by the page's owner, not here, so a screen that SwiftUI draws again
/// shows the same web view and the page in it is not loaded again.
struct GlasshouseWebContainer: UIViewRepresentable {
    let webView: WKWebView
    /// Whether the page is in restricted mode. The page is always in it; the layer is put back when it is not.
    var restricted = true
    /// Told whether the page is out of screenshots, each time the host tries to put it there.
    let screenshotProtection: (Bool) -> Void

    func makeUIView(context: Context) -> GlasshouseWebHost {
        GlasshouseWebHost(webView: webView)
    }

    func updateUIView(_ host: GlasshouseWebHost, context: Context) {
        host.screenshotProtection = screenshotProtection
        host.restricted = restricted
    }

    static func dismantleUIView(_ host: GlasshouseWebHost, coordinator: ()) {
        host.restricted = false
    }
}

/// The view SwiftUI shows for the page. The web view sits in a content view that fills the host, and the layer of
/// that content view is the one moved into the secure canvas while the host is in a window and the page is in
/// restricted mode. The view hierarchy does not change, so touches, scrolling, text selection and layout are the web
/// view's own.
@MainActor
final class GlasshouseWebHost: UIView {
    private let content = UIView()
    private let webView: WKWebView
    private let shield: GlasshouseScreenshotShield

    /// Told whether the page is out of screenshots after each try to put it there.
    var screenshotProtection: (Bool) -> Void = { _ in }

    var restricted = true {
        didSet {
            guard restricted != oldValue else { return }
            updateProtection()
        }
    }

    init(webView: WKWebView, shield: GlasshouseScreenshotShield? = nil) {
        self.webView = webView
        self.shield = shield ?? GlasshouseScreenshotShield()
        super.init(frame: .zero)
        addSubview(content)
        webView.removeFromSuperview()
        content.addSubview(webView)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not used")
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        updateProtection()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        // Sized here and not by autoresizing masks: a resize is then one layout, whatever layer the content is in.
        content.frame = bounds
        webView.frame = content.bounds
        shield.parentResized()
    }

    /// The layer is in the canvas while the host is on the screen in restricted mode, and in its own place otherwise.
    private func updateProtection() {
        if restricted && window != nil {
            screenshotProtection(shield.protect(content.layer))
        } else {
            shield.restore()
        }
    }
}
