@testable import Construct
import UIKit
import XCTest

/// Keeping a layer out of screenshots: the layer move of `GlasshouseScreenshotShield` on plain layers and on the
/// system's own secure field. The Simulator draws what is in the canvas layer as it draws everything else, so none of
/// these can show that a screenshot is black; they show that the layer tree is changed and changed back the way it
/// should be.
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

    // MARK: A layer that something else took out of the canvas

    func testALayerThatWasTakenOutOfTheCanvasIsLeftWhereItIsAndTheFieldGoes() {
        let tree = Tree()
        let field = FakeField()
        let (shield, _) = shield(field)
        shield.protect(tree.content)
        tree.content.removeFromSuperlayer()
        XCTAssertFalse(shield.isProtecting)
        shield.restore()
        XCTAssertNil(tree.content.superlayer, "a layer its view had removed was put back")
        XCTAssertEqual(tree.children, [tree.before, tree.after])
        XCTAssertNil(field.layer.superlayer)
    }

    func testProtectingAgainMovesALayerThatWasTakenOutAndPutBackByItsView() {
        let tree = Tree()
        let first = FakeField()
        let second = FakeField()
        let (shield, made) = shield(first, second)
        shield.protect(tree.content)
        tree.content.removeFromSuperlayer()
        tree.parent.insertSublayer(tree.content, at: 1)
        XCTAssertTrue(shield.protect(tree.content))
        XCTAssertEqual(made(), 2)
        XCTAssertTrue(tree.content.superlayer === second.canvasLayer)
        XCTAssertNil(first.layer.superlayer, "the first field stayed")
        XCTAssertEqual(tree.children, [tree.before, second.layer, tree.after])
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
