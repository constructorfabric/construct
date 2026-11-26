@testable import Construct
import UIKit
import WebKit
import XCTest

/// A layer that counts the times it was asked to draw again.
private final class CountingLayer: CALayer {
    private(set) var redraws = 0

    override func setNeedsDisplay() {
        redraws += 1
        super.setNeedsDisplay()
    }
}

/// A field made of plain layers, with a canvas layer or, as when the system changes, without one.
@MainActor
private final class FakeField: SecureCanvasField {
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
private struct Tree {
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

/// What a host told about the page being out of screenshots.
private final class Told {
    var results: [Bool] = []
}

/// A host with a web view in a window, the way the page has it, and what the host told.
@MainActor
private struct Page {
    let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
    let webView = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
    let host: GlasshouseWebHost
    let told: Told

    init(shield: GlasshouseScreenshotShield? = nil) {
        host = GlasshouseWebHost(webView: webView, shield: shield)
        let told = Told()
        self.told = told
        host.screenshotProtection = { told.results.append($0) }
        host.frame = CGRect(x: 0, y: 100, width: 390, height: 600)
    }

    /// The view that holds the web view, whose layer is the one that moves.
    var content: UIView { webView.superview ?? UIView() }

    func show() {
        window.addSubview(host)
        host.layoutIfNeeded()
    }

    func leave() {
        host.removeFromSuperview()
    }
}

/// Keeping the page out of screenshots: the layer move of `GlasshouseScreenshotShield` on plain layers, on the
/// system's own secure field, and the host view that does it for the web view. The Simulator draws what is in the
/// canvas layer as it draws everything else, so none of these can show that a screenshot is black; they show that the
/// layer tree is changed and changed back the way it should be.
@MainActor
final class GlasshouseScreenshotShieldTests: XCTestCase {
    private func shield(_ fields: FakeField...) -> (GlasshouseScreenshotShield, made: () -> Int) {
        var queue = fields
        var count = 0
        let shield = GlasshouseScreenshotShield {
            count += 1
            return queue.isEmpty ? FakeField() : queue.removeFirst()
        }
        return (shield, { count })
    }

    // MARK: The move on plain layers

    func testTheLayerGoesIntoTheCanvasAndTheFieldTakesItsPlaceUnderTheSameParent() {
        let tree = Tree()
        let field = FakeField()
        let (shield, _) = shield(field)
        XCTAssertTrue(shield.protect(tree.content))
        XCTAssertTrue(shield.isProtecting)
        XCTAssertTrue(tree.content.superlayer === field.canvasLayer)
        XCTAssertTrue(field.canvasLayer?.superlayer === field.layer)
        XCTAssertEqual(tree.children, [tree.before, field.layer, tree.after])
        XCTAssertEqual(field.fitted, tree.parent.bounds)
    }

    func testProtectingTwiceChangesNothing() {
        let tree = Tree()
        let (shield, made) = shield(FakeField())
        XCTAssertTrue(shield.protect(tree.content))
        let after = tree.children
        XCTAssertTrue(shield.protect(tree.content))
        XCTAssertEqual(tree.children, after)
        XCTAssertEqual(made(), 1, "a second field was made")
        XCTAssertTrue(shield.isProtecting)
    }

    func testRestoringPutsTheLayerBackAtItsPlaceAndTheFieldGoes() {
        let tree = Tree()
        let field = FakeField()
        let (shield, _) = shield(field)
        shield.protect(tree.content)
        shield.restore()
        XCTAssertFalse(shield.isProtecting)
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
        XCTAssertNil(field.layer.superlayer)
        XCTAssertTrue(tree.content.superlayer === tree.parent)
    }

    func testRestoringTwiceOrBeforeAnyMoveChangesNothing() {
        let tree = Tree()
        let (shield, _) = shield(FakeField())
        shield.restore()
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
        shield.protect(tree.content)
        shield.restore()
        shield.restore()
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
    }

    func testWithNoCanvasLayerNothingChangesAndTheTryIsNotLatched() {
        let tree = Tree()
        let (shield, _) = shield(FakeField(canvas: false), FakeField())
        XCTAssertFalse(shield.protect(tree.content))
        XCTAssertFalse(shield.isProtecting)
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
        shield.restore()
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
        XCTAssertTrue(shield.protect(tree.content), "the second try was refused after the first one failed")
        XCTAssertTrue(shield.isProtecting)
    }

    func testALayerWithNoParentIsNotProtectedAndNoFieldIsMade() {
        let layer = CALayer()
        let (shield, made) = shield(FakeField())
        XCTAssertFalse(shield.protect(layer))
        XCTAssertFalse(shield.isProtecting)
        XCTAssertNil(layer.superlayer)
        XCTAssertEqual(made(), 0)
    }

    func testTheLayerThatWasMovedIsPutBackNotTheOneProtectedNow() {
        let first = Tree()
        let second = Tree()
        let (shield, _) = shield(FakeField(), FakeField())
        shield.protect(first.content)
        shield.protect(second.content)
        XCTAssertEqual(first.children.count, 3)
        XCTAssertTrue(first.content.superlayer === first.parent, "the first layer was left in the canvas")
        XCTAssertFalse(second.content.superlayer === second.parent)
        shield.restore()
        XCTAssertEqual(second.children, [second.before, second.content, second.after])
        XCTAssertEqual(first.children, [first.before, first.content, first.after])
    }

    func testTheFieldFollowsTheParentsSize() {
        let tree = Tree()
        let field = FakeField()
        let (shield, _) = shield(field)
        shield.parentResized()
        XCTAssertEqual(field.fitted, .zero, "a field that was not made was fitted")
        shield.protect(tree.content)
        tree.parent.bounds = CGRect(x: 0, y: 0, width: 500, height: 300)
        shield.parentResized()
        XCTAssertEqual(field.fitted, tree.parent.bounds)
    }

    // MARK: Drawing again

    func testTheMovedLayerTreeIsAskedToDrawAgainRightAfterOnTheNextTurnAndATenthOfASecondLater() async throws {
        let content = CountingLayer()
        let child = CountingLayer()
        content.addSublayer(child)
        let tree = Tree(content: content)
        let (shield, _) = shield(FakeField())
        shield.protect(tree.content)
        XCTAssertEqual([content.redraws, child.redraws], [1, 1], "no redraw right after the move")
        try await Task.sleep(nanoseconds: 20_000_000)
        XCTAssertGreaterThanOrEqual(content.redraws, 2, "no redraw on the next turn")
        XCTAssertGreaterThanOrEqual(child.redraws, 2, "the redraw did not reach the sublayer")
        try await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertEqual([content.redraws, child.redraws], [3, 3], "no redraw a tenth of a second later")
    }

    func testPuttingTheLayerBackAsksForTheSameRedraws() async throws {
        let content = CountingLayer()
        let tree = Tree(content: content)
        let (shield, _) = shield(FakeField())
        shield.protect(tree.content)
        try await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertEqual(content.redraws, 3)
        shield.restore()
        XCTAssertEqual(content.redraws, 4, "no redraw right after the layer was put back")
        try await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertEqual(content.redraws, 6)
    }

    func testATryThatFailedAsksForNoRedraw() async throws {
        let content = CountingLayer()
        let tree = Tree(content: content)
        let (shield, _) = shield(FakeField(canvas: false))
        shield.protect(tree.content)
        try await Task.sleep(nanoseconds: 300_000_000)
        XCTAssertEqual(content.redraws, 0)
    }

    // MARK: The system's own secure field

    func testTheSystemsSecureFieldGivesACanvasLayerAndTheLayerKeepsItsPlaceInIt() throws {
        let tree = Tree()
        tree.content.frame = CGRect(x: 10, y: 20, width: 100, height: 200)
        let shield = GlasshouseScreenshotShield()
        XCTAssertTrue(shield.protect(tree.content), "this system's secure field gave no canvas layer")
        let canvas = try XCTUnwrap(tree.content.superlayer)
        XCTAssertFalse(canvas === tree.parent)
        XCTAssertEqual(tree.parent.convert(CGPoint.zero, from: tree.content), CGPoint(x: 10, y: 20))
        XCTAssertEqual(tree.content.frame, CGRect(x: 10, y: 20, width: 100, height: 200))
        shield.restore()
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after])
    }
}

/// The host that holds the web view and moves its layer into the secure canvas while it is on the screen.
@MainActor
final class GlasshouseWebHostTests: XCTestCase {
    func testTheHostMovesTheLayerWhenItIsOnTheScreenAndPutsItBackWhenItLeaves() {
        let page = Page()
        XCTAssertTrue(page.content.layer.superlayer === page.host.layer, "moved before the page was on the screen")
        XCTAssertEqual(page.told.results, [])
        page.show()
        XCTAssertEqual(page.told.results, [true])
        XCTAssertFalse(page.content.layer.superlayer === page.host.layer)
        page.leave()
        XCTAssertTrue(page.content.layer.superlayer === page.host.layer, "left in the canvas after the page left")
        XCTAssertEqual(page.host.layer.sublayers?.count, 1)
        page.show()
        XCTAssertEqual(page.told.results, [true, true])
        XCTAssertFalse(page.content.layer.superlayer === page.host.layer)
    }

    func testTheHostPutsTheLayerBackWhenRestrictedModeEndsAndMovesItWhenItReturns() {
        let page = Page()
        page.show()
        page.host.restricted = false
        XCTAssertTrue(page.content.layer.superlayer === page.host.layer)
        XCTAssertEqual(page.host.layer.sublayers?.count, 1)
        page.host.restricted = true
        XCTAssertFalse(page.content.layer.superlayer === page.host.layer)
        XCTAssertEqual(page.told.results, [true, true])
    }

    func testTheWebViewFillsTheHostAndKeepsItsPlaceInTheCanvasWhenTheHostChangesSize() throws {
        let page = Page()
        page.show()
        XCTAssertEqual(page.webView.frame.size, CGSize(width: 390, height: 600))
        let canvas = try XCTUnwrap(page.content.layer.superlayer)
        XCTAssertEqual(page.host.layer.convert(CGPoint.zero, from: page.content.layer), .zero)

        page.host.frame = CGRect(x: 0, y: 100, width: 844, height: 300)
        page.host.layoutIfNeeded()
        XCTAssertEqual(page.webView.frame.size, CGSize(width: 844, height: 300))
        XCTAssertEqual(page.content.frame, page.host.bounds)
        XCTAssertEqual(canvas.superlayer?.bounds.size, CGSize(width: 844, height: 300), "the field did not follow")
        XCTAssertEqual(page.host.layer.convert(CGPoint.zero, from: page.content.layer), .zero)
    }

    func testAHostWhoseSystemGivesNoCanvasLayerTellsSoAndLeavesTheHierarchyAsItWas() {
        let page = Page(shield: GlasshouseScreenshotShield { FakeField(canvas: false) })
        page.show()
        XCTAssertEqual(page.told.results, [false])
        XCTAssertTrue(page.content.layer.superlayer === page.host.layer)
        XCTAssertEqual(page.host.layer.sublayers?.count, 1)
        XCTAssertTrue(page.webView.superview === page.content)
        XCTAssertTrue(page.content.superview === page.host)
    }

    func testTheStateHolderFollowsWhatTheHostTold() {
        let restriction = GlasshouseRestriction(center: NotificationCenter(), appActive: true, isCaptured: { false })
        let page = Page(shield: GlasshouseScreenshotShield { FakeField(canvas: false) })
        page.host.screenshotProtection = { restriction.screenshotProtection(worked: $0) }
        XCTAssertFalse(restriction.state.screenshotNotice, "a line before any try")
        page.show()
        XCTAssertFalse(restriction.state.screenshotProtected)
        XCTAssertTrue(restriction.state.screenshotNotice, "no line after a try that failed")

        let working = Page()
        working.host.screenshotProtection = { restriction.screenshotProtection(worked: $0) }
        working.show()
        XCTAssertTrue(restriction.state.screenshotProtected)
        XCTAssertFalse(restriction.state.screenshotNotice, "the line stays where the page is protected")
    }
}
