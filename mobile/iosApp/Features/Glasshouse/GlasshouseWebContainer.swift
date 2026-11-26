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

/// The view SwiftUI shows for the page, with the web view filling it. While the host is in a window and the page is in
/// restricted mode, the layer of that window is moved into the secure canvas, so what the window draws is out of
/// screenshots and recordings. The view hierarchy does not change, so touches, scrolling, text selection and layout are
/// the web view's own.
///
/// It is the window's layer, and not the page's, because a web view takes no keyboard focus while the layer of one of
/// its views, or of a view above it, is out of its superview's layer: UIKit refuses `becomeFirstResponder()`, and
/// nothing can be typed into a field of the page. Moving the layer of a view inside the window also makes UIKit take
/// the secure field for one of the views, and removing that view's parent from the window crashes it. The window has no
/// superview, so its layer can move without either. The cost is that everything the window shows is out of
/// screenshots while the page is on the screen, not the page alone.
@MainActor
final class GlasshouseWebHost: UIView {
    private let webView: WKWebView
    private let shield: GlasshouseScreenshotShield
    private let layerToProtect: @MainActor (UIWindow) -> CALayer?

    /// Told whether the page is out of screenshots after each try to put it there.
    var screenshotProtection: (Bool) -> Void = { _ in }

    var restricted = true {
        didSet {
            guard restricted != oldValue else { return }
            updateProtection()
        }
    }

    /// [layerToProtect] says which layer of the window is moved; the window's own, unless a test says another.
    init(
        webView: WKWebView,
        shield: GlasshouseScreenshotShield? = nil,
        layerToProtect: @escaping @MainActor (UIWindow) -> CALayer? = { $0.layer }
    ) {
        self.webView = webView
        self.shield = shield ?? GlasshouseScreenshotShield()
        self.layerToProtect = layerToProtect
        super.init(frame: .zero)
        webView.removeFromSuperview()
        addSubview(webView)
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
        webView.frame = bounds
        shield.parentResized()
    }

    /// The layer is in the canvas while the host is on the screen in restricted mode, and in its own place otherwise. A
    /// layer with no parent layer cannot be protected, and the host says so.
    private func updateProtection() {
        guard restricted, let window else {
            shield.restore()
            return
        }
        screenshotProtection(layerToProtect(window).map { shield.protect($0) } ?? false)
    }
}
