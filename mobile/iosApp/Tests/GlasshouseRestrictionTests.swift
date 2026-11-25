@testable import Construct
import UIKit
import XCTest

/// The page's restricted mode while the app runs: the clipboard guard follows the page and the app, a recording
/// hides the page, and a refused action is told for a while.
@MainActor
final class GlasshouseRestrictionTests: XCTestCase {
    private let center = NotificationCenter()
    private var captured = false

    private final class Board: GlasshousePasteboard {
        var held: [[String: Data]] = []
        func read() -> [[String: Data]]? { held }
        func replace(with items: [[String: Data]]) { held = items }
        func copy(_ text: String) { held = [["public.utf8-plain-text": Data(text.utf8)]] }
        var text: String? { held.first?["public.utf8-plain-text"].flatMap { String(data: $0, encoding: .utf8) } }
    }

    /// The page copies [text]: the pasteboard changes, and the system says so.
    private func pageCopies(_ text: String, on board: Board) {
        board.copy(text)
        center.post(name: UIPasteboard.changedNotification, object: nil)
    }

    private func restriction(
        _ board: Board = Board(), blockable: Bool = false, active: Bool = true, seconds: Double = 6
    ) -> GlasshouseRestriction {
        GlasshouseRestriction(
            screenshotCanBeBlocked: blockable,
            clipboard: GlasshouseClipboard(pasteboard: board),
            center: center,
            appActive: active,
            isCaptured: { self.captured },
            noticeSeconds: seconds
        )
    }

    // MARK: The line about screenshots

    func testALineSaysAScreenshotCannotBeBlockedWhereTheWayIsNotThereAndGoesWhenTheyReadIt() {
        let restriction = restriction()
        XCTAssertTrue(restriction.state.screenshotNotice)
        restriction.dismissScreenshotNotice()
        XCTAssertFalse(restriction.state.screenshotNotice)
    }

    func testNoLineWhereAScreenshotCanBeBlocked() {
        XCTAssertFalse(restriction(blockable: true).state.screenshotNotice)
    }

    func testThisBuildDoesNotRelyOnTheSecureFieldWayAndSaysSo() {
        XCTAssertFalse(GlasshouseScreenshot.canBeBlocked)
        XCTAssertTrue(restriction().state.screenshotNotice)
    }

    // MARK: A recording or another screen

    func testThePageIsHiddenWhileTheScreenIsRecordedAndShownAgainAfterwards() {
        let restriction = restriction()
        restriction.pageAppeared()
        XCTAssertFalse(restriction.state.covered)
        captured = true
        center.post(name: UIScreen.capturedDidChangeNotification, object: nil)
        XCTAssertTrue(restriction.state.covered)
        captured = false
        center.post(name: UIScreen.capturedDidChangeNotification, object: nil)
        XCTAssertFalse(restriction.state.covered)
    }

    func testThePageIsHiddenFromTheFirstMomentWhenTheScreenIsAlreadyRecorded() {
        captured = true
        let restriction = restriction()
        restriction.pageAppeared()
        XCTAssertTrue(restriction.state.covered)
    }

    func testAPageThatIsNotOnTheScreenHidesNothing() {
        captured = true
        let restriction = restriction()
        restriction.pageAppeared()
        restriction.pageDisappeared()
        XCTAssertFalse(restriction.state.covered)
    }

    // MARK: The clipboard follows the page and the app

    func testWhatThePageCopiedIsGoneWhenTheAppResignsAndWhatWasThereBeforeIsBack() {
        let board = Board()
        board.copy("from another app")
        let restriction = restriction(board)
        restriction.pageAppeared()
        pageCopies("page text", on: board)
        center.post(name: UIApplication.willResignActiveNotification, object: nil)
        XCTAssertEqual(board.text, "from another app")
    }

    func testWhatThePageCopiedIsGoneWhenThePersonGoesToAnotherPageOfTheApp() {
        let board = Board()
        board.copy("from another app")
        let restriction = restriction(board)
        restriction.pageAppeared()
        pageCopies("page text", on: board)
        restriction.pageDisappeared()
        XCTAssertEqual(board.text, "from another app")
    }

    func testTheAppComingBackToTheFrontStartsGuardingAgainWithWhatIsThenOnThePasteboard() {
        let board = Board()
        board.copy("first")
        let restriction = restriction(board)
        restriction.pageAppeared()
        center.post(name: UIApplication.willResignActiveNotification, object: nil)
        board.copy("copied in another app")
        center.post(name: UIApplication.didBecomeActiveNotification, object: nil)
        pageCopies("page text", on: board)
        center.post(name: UIApplication.willResignActiveNotification, object: nil)
        XCTAssertEqual(board.text, "copied in another app")
    }

    func testAPageShownWhileTheAppIsNotInFrontStartsGuardingWhenItComesToTheFront() {
        let board = Board()
        board.copy("from another app")
        let restriction = restriction(board, active: false)
        restriction.pageAppeared()
        center.post(name: UIApplication.didBecomeActiveNotification, object: nil)
        pageCopies("page text", on: board)
        restriction.pageDisappeared()
        XCTAssertEqual(board.text, "from another app")
    }

    // MARK: A refused action

    func testARefusedActionIsToldAndTheLineGoesAwayAfterAFewSeconds() async throws {
        let restriction = restriction(seconds: 0.2)
        restriction.blocked(.download)
        XCTAssertEqual(restriction.state.blocked, .download)
        try await Task.sleep(nanoseconds: 600_000_000)
        XCTAssertNil(restriction.state.blocked)
    }

    func testANewRefusedActionReplacesTheLineAndKeepsItFullTime() async throws {
        let restriction = restriction(seconds: 0.5)
        restriction.blocked(.print)
        try await Task.sleep(nanoseconds: 300_000_000)
        restriction.blocked(.share)
        try await Task.sleep(nanoseconds: 300_000_000)
        XCTAssertEqual(restriction.state.blocked, .share, "the first line's time ended this one")
        try await Task.sleep(nanoseconds: 500_000_000)
        XCTAssertNil(restriction.state.blocked)
    }

    func testEveryRefusedActionHasItsOwnLineThatSaysItIsRestrictedMode() {
        for action in [GlasshouseBlockedAction.download, .print, .share] {
            XCTAssertTrue(GlasshouseTexts.blocked(action).hasSuffix("is blocked in restricted mode."), "\(action)")
        }
        XCTAssertEqual(Set([GlasshouseBlockedAction.download, .print, .share].map(GlasshouseTexts.blocked)).count, 3)
    }
}
