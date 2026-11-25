@testable import Construct
import UIKit
import XCTest

/// A pasteboard that counts what is put on it, and can hold something that this app is not let read.
private final class FakePasteboard: GlasshousePasteboard {
    private(set) var held: [[String: Data]] = []
    private(set) var writes = 0
    private var readable = true

    func read() -> [[String: Data]]? {
        readable ? held : nil
    }

    func replace(with items: [[String: Data]]) {
        held = items
        readable = true
        writes += 1
    }

    /// The app puts a text on the pasteboard: the page's own copy command, or the app's.
    func copy(_ text: String) {
        replace(with: [["public.utf8-plain-text": Data(text.utf8)]])
        writes -= 1
    }

    /// Another app, or another device, puts a text on the pasteboard, which this app is not let read.
    func copyInAnotherApp(_ text: String) {
        held = [["public.utf8-plain-text": Data(text.utf8)]]
        readable = false
    }

    var text: String? {
        held.first?["public.utf8-plain-text"].flatMap { String(data: $0, encoding: .utf8) }
    }
}

@MainActor
final class GlasshouseClipboardTests: XCTestCase {
    private let pasteboard = FakePasteboard()
    private lazy var clipboard = GlasshouseClipboard(pasteboard: pasteboard)

    /// The page copies [text]: the pasteboard changes and the guard is told.
    private func pageCopies(_ text: String) {
        pasteboard.copy(text)
        clipboard.pasteboardChanged()
    }

    func testWhatThePageCopiesIsReplacedByWhatWasThereBeforeWhenThePageIsLeft() {
        pasteboard.copy("copied in the app")
        clipboard.pageIsShown()
        pageCopies("page text")
        XCTAssertEqual(pasteboard.text, "page text", "the page's text is there for the page to paste")
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.text, "copied in the app")
    }

    func testAnEmptyPasteboardIsEmptyAgainWhenThePageIsLeft() {
        clipboard.pageIsShown()
        pageCopies("page text")
        clipboard.pageIsLeft()
        XCTAssertTrue(pasteboard.held.isEmpty)
    }

    func testWhatWasThereBeforeIsPutBackOnlyOnceAndTheNextTimeThePageIsShownTakesItAgain() {
        pasteboard.copy("first")
        clipboard.pageIsShown()
        pageCopies("page text")
        clipboard.pageIsLeft()
        let writes = pasteboard.writes
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.writes, writes)
        pasteboard.copy("second, copied in the app")
        clipboard.pageIsShown()
        pageCopies("more page text")
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.text, "second, copied in the app")
    }

    func testAPasteboardThePageDidNotChangeIsLeftAlone() {
        pasteboard.copy("copied in the app")
        clipboard.pageIsShown()
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.writes, 0)
        XCTAssertEqual(pasteboard.text, "copied in the app")
    }

    func testWhatWasThereBeforeIsGoneWhenTheAppWasNotLetReadItAndOnlyThePagesTextIsTakenAway() {
        pasteboard.copyInAnotherApp("from another app")
        clipboard.pageIsShown()
        pageCopies("page text")
        clipboard.pageIsLeft()
        XCTAssertTrue(pasteboard.held.isEmpty)
    }

    func testWhatAnotherAppPutOnThePasteboardWhileThePageWasShownIsLeftAlone() {
        pasteboard.copy("copied in the app")
        clipboard.pageIsShown()
        pasteboard.copyInAnotherApp("from another app")
        clipboard.pasteboardChanged()
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.writes, 0)
        XCTAssertEqual(pasteboard.text, "from another app")
    }

    func testAChangeWhileThePageIsNotShownIsNotThePages() {
        pasteboard.copy("copied in the app")
        clipboard.pasteboardChanged()
        clipboard.pageIsShown()
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.writes, 0)
    }

    func testShowingThePageTwiceDoesNotTakeThePagesTextForWhatWasThereBefore() {
        pasteboard.copy("copied in the app")
        clipboard.pageIsShown()
        pageCopies("page text")
        clipboard.pageIsShown()
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.text, "copied in the app")
    }

    func testLeavingAPageThatWasNotShownDoesNothing() {
        pasteboard.copy("copied in the app")
        clipboard.pageIsLeft()
        pageCopies("copied elsewhere")
        clipboard.pageIsLeft()
        XCTAssertEqual(pasteboard.text, "copied elsewhere")
        XCTAssertEqual(pasteboard.writes, 0)
    }

    func testThePasteboardOfTheSystemTakesItemsAndGivesBackWhatTheAppPutThere() throws {
        let system = SystemPasteboard()
        // What another app left there cannot be read, and so cannot be put back; the test does not need it.
        let before = system.read()
        defer { system.replace(with: before ?? []) }
        let items = [["public.utf8-plain-text": Data("kept".utf8)], ["public.url": Data("https://example.test".utf8)]]
        system.replace(with: items)
        let held = try XCTUnwrap(system.read())
        XCTAssertEqual(held.count, 2)
        XCTAssertEqual(held.first?["public.utf8-plain-text"], Data("kept".utf8))
        XCTAssertEqual(held.last?["public.url"], Data("https://example.test".utf8))
        system.replace(with: [])
        XCTAssertEqual(try XCTUnwrap(system.read()).count, 0)
    }
}
