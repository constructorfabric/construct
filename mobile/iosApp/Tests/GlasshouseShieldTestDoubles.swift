@testable import Construct
import UIKit
import WebKit

/// A layer that counts the times it was asked to draw again.
final class CountingLayer: CALayer {
    private(set) var redraws = 0

    override func setNeedsDisplay() {
        redraws += 1
        super.setNeedsDisplay()
    }
}

/// A field made of plain layers, with a canvas layer or, as when the system changes, without one.
@MainActor
final class FakeField: SecureCanvasField {
    let layer = CALayer()
    let canvasLayer: CALayer?
    private(set) var fitted = CGRect.zero

    init(canvas: Bool = true) {
        canvasLayer = canvas ? CALayer() : nil
        if let canvasLayer { layer.addSublayer(canvasLayer) }
    }

    func fit(to bounds: CGRect) {
        fitted = bounds
    }
}

/// A parent with the layer to protect between two others.
@MainActor
struct Tree {
    let parent = CALayer()
    let before = CALayer()
    let content: CALayer
    let after = CALayer()

    init(content: CALayer = CALayer()) {
        self.content = content
        parent.bounds = CGRect(x: 0, y: 0, width: 300, height: 500)
        [before, content, after].forEach(parent.addSublayer)
    }

    var children: [CALayer] { parent.sublayers ?? [] }
}

/// What a host told about the page being out of screenshots: each try's result, and how many times it said the
/// protection ended.
final class Told {
    var results: [Bool] = []
    var ended = 0
}

/// A host with a web view, a window it can be shown in, a layer tree standing in for the window's layer, and what the
/// host told. The layer that moves is the one in the tree unless a test says another, so the real window's layer is not
/// taken out of what holds it.
@MainActor
struct HostedPage {
    let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
    let tree: Tree
    let webView = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
    let host: GlasshouseWebHost
    let told: Told

    init(shield: GlasshouseScreenshotShield? = nil, layer: (@MainActor (UIWindow) -> CALayer?)? = nil) {
        let tree = Tree()
        self.tree = tree
        host = GlasshouseWebHost(webView: webView, shield: shield, layerToProtect: layer ?? { _ in tree.content })
        let told = Told()
        self.told = told
        host.screenshotProtection = { told.results.append($0) }
        host.screenshotProtectionEnded = { told.ended += 1 }
        host.frame = CGRect(x: 0, y: 100, width: 390, height: 600)
    }

    func show() {
        window.addSubview(host)
        host.layoutIfNeeded()
    }

    func leave() {
        host.removeFromSuperview()
    }
}
