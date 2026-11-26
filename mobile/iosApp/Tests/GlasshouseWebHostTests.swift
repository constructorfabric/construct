@testable import Construct
import UIKit
import WebKit
import XCTest

/// The host that holds the web view and moves a layer of its window into the secure canvas while it is on the screen.
@MainActor
final class GlasshouseWebHostTests: XCTestCase {
    private func makePage(
        shield: GlasshouseScreenshotShield? = nil,
        layer: (@MainActor (UIWindow) -> CALayer?)? = nil
    ) -> HostedPage {
        let page = HostedPage(shield: shield, layer: layer)
        addTeardownBlock {
            MainActor.assumeIsolated {
                page.host.restricted = false
                page.leave()
            }
        }
        return page
    }

    private func assertInPlace(_ page: HostedPage, _ message: String = "", line: UInt = #line) {
        let tree = page.tree
        XCTAssertEqual(tree.children, [tree.before, tree.content, tree.after], message, line: line)
    }

    func testTheLayerMovesIntoTheCanvasWhenTheHostIsOnAWindowAndComesBackWhenItLeaves() {
        let page = makePage()
        XCTAssertEqual(page.told.results, [])
        assertInPlace(page, "moved before the page was on a window")
        page.show()
        XCTAssertEqual(page.told.results, [true])
        XCTAssertFalse(page.tree.content.superlayer === page.tree.parent)
        XCTAssertEqual(page.tree.children.count, 3, "the field did not take the layer's place")
        page.leave()
        assertInPlace(page, "the layer did not come back")
        page.show()
        XCTAssertEqual(page.told.results, [true, true])
        XCTAssertFalse(page.tree.content.superlayer === page.tree.parent)
    }

    func testTheLayerComesBackWhenRestrictedModeEndsAndMovesAgainWhenItReturns() {
        let page = makePage()
        page.show()
        page.host.restricted = false
        assertInPlace(page)
        page.host.restricted = true
        XCTAssertFalse(page.tree.content.superlayer === page.tree.parent)
        XCTAssertEqual(page.told.results, [true, true])
    }

    func testTheHostTellsTheProtectionEndedWhenItLeavesAndWhenRestrictedModeEnds() {
        let page = makePage()
        page.show()
        XCTAssertEqual(page.told.ended, 0, "told before the layer was put back")
        page.leave()
        XCTAssertEqual(page.told.ended, 1)
        page.show()
        page.host.restricted = false
        XCTAssertEqual(page.told.ended, 2)
        XCTAssertEqual(page.told.results, [true, true], "a put back was told as a try")
    }

    func testSwiftUIDismantlingTheHostPutsTheLayerBackWhileTheHostIsStillOnTheWindow() {
        let page = makePage()
        page.show()
        XCTAssertFalse(page.tree.content.superlayer === page.tree.parent)
        GlasshouseWebContainer.dismantleUIView(page.host, coordinator: ())
        XCTAssertTrue(page.host.window === page.window, "the host left the window by itself")
        assertInPlace(page, "the layer stayed in the canvas")
        XCTAssertEqual(page.told.ended, 1)
    }

    func testTheHostGivesTheLayerOfItsOwnWindowToProtect() {
        var seen: UIWindow?
        let page = makePage { window in
            seen = window
            return nil
        }
        page.show()
        XCTAssertTrue(seen === page.window)
        XCTAssertEqual(page.told.results, [false], "a window with no layer to protect was told as protected")
    }

    func testTheWebViewFillsTheHostAndTheHostKeepsItsPlaceWhenItChangesSize() {
        let page = makePage()
        page.show()
        XCTAssertEqual(page.webView.frame.size, CGSize(width: 390, height: 600))
        XCTAssertEqual(page.window.layer.convert(CGPoint.zero, from: page.host.layer), CGPoint(x: 0, y: 100))

        page.host.frame = CGRect(x: 0, y: 100, width: 844, height: 300)
        page.host.layoutIfNeeded()
        XCTAssertEqual(page.webView.frame, page.host.bounds)
        XCTAssertEqual(page.window.layer.convert(CGPoint.zero, from: page.host.layer), CGPoint(x: 0, y: 100))
    }

    func testTheFieldFollowsTheSizeOfTheParentWhenTheHostIsLaidOut() {
        let field = FakeField()
        let page = makePage(shield: GlasshouseScreenshotShield { field })
        page.show()
        XCTAssertEqual(field.fitted, page.tree.parent.bounds)
        page.tree.parent.bounds = CGRect(x: 0, y: 0, width: 844, height: 390)
        page.host.setNeedsLayout()
        page.host.layoutIfNeeded()
        XCTAssertEqual(field.fitted, CGRect(x: 0, y: 0, width: 844, height: 390), "the field did not follow")
    }

    func testAHostWhoseSystemGivesNoCanvasLayerTellsSoAndLeavesTheTreeAsItWas() {
        let page = makePage(shield: GlasshouseScreenshotShield { FakeField(canvas: false) })
        page.show()
        XCTAssertEqual(page.told.results, [false])
        assertInPlace(page)
        XCTAssertTrue(page.webView.superview === page.host)
        XCTAssertTrue(page.host.superview === page.window)
    }

    func testTheStateHolderFollowsWhatTheHostTold() {
        let restriction = GlasshouseRestriction(center: NotificationCenter(), appActive: true, isCaptured: { false })
        let page = makePage(shield: GlasshouseScreenshotShield { FakeField(canvas: false) })
        page.host.screenshotProtection = { restriction.screenshotProtection(worked: $0) }
        XCTAssertFalse(restriction.state.screenshotNotice, "a line before any try")
        page.show()
        XCTAssertFalse(restriction.state.screenshotProtected)
        XCTAssertTrue(restriction.state.screenshotNotice, "no line after a try that failed")

        let working = makePage()
        working.host.screenshotProtection = { restriction.screenshotProtection(worked: $0) }
        working.show()
        XCTAssertTrue(restriction.state.screenshotProtected)
        XCTAssertFalse(restriction.state.screenshotNotice, "the line stays where the page is protected")
    }

    func testTheSignStopsSayingHiddenWhenTheHostLeavesAndSaysItAgainWhenTheHostComesBack() {
        let restriction = GlasshouseRestriction(center: NotificationCenter(), appActive: true, isCaptured: { false })
        let page = makePage()
        page.host.screenshotProtection = { restriction.screenshotProtection(worked: $0) }
        page.host.screenshotProtectionEnded = { restriction.screenshotProtectionEnded() }
        page.show()
        XCTAssertTrue(restriction.state.screenshotProtected)
        page.leave()
        XCTAssertFalse(restriction.state.screenshotProtected, "the sign said hidden while the page was not")
        XCTAssertFalse(restriction.state.screenshotNotice, "leaving raised the line")
        page.show()
        XCTAssertTrue(restriction.state.screenshotProtected)
    }
}
