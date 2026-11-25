import SwiftUI
import WebKit

/// Puts the page's one web view on the screen. The web view is made and kept by the page's owner, not here, so a
/// screen that SwiftUI draws again shows the same web view and the page in it is not loaded again.
struct GlasshouseWebContainer: UIViewRepresentable {
    let webView: WKWebView

    func makeUIView(context: Context) -> WKWebView {
        webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}
}
