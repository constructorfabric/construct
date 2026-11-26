import XCTest

/// Restricted mode on the Glasshouse page: the sign and the line about screenshots, the touch menu, the clipboard,
/// and the refusal of a download, a print and a share. They run against the test server of
/// `scripts/test-glasshouse-ios.py`, which also sets and reads the Simulator's pasteboard for them.
final class GlasshouseRestrictedTests: GlasshouseTestCase {
    private let beforeText = "text from another app"
    private let pageText = "page-secret-code"
    private let refusedActions = [
        "Copy", "Cut", "Share", "Share…", "Copy Link with Highlight", "Save to Photos", "Save Image", "Copy Image"
    ]

    /// Opens the test page of restricted mode. [pasteboardText] is what the app puts on the pasteboard as it starts, as
    /// what the person copied before.
    private func openRestrictedPage(pasteboardText: String? = nil) {
        launch(pasteboardText: pasteboardText)
        openGlasshouse()
        go(site.trusted + "/restricted")
        waitForPage("Restricted notes")
    }

    private func sign() -> XCUIElement {
        app.descendants(matching: .any)["glasshouse-restricted"]
    }

    /// Puts [text] on the pasteboard from outside the app, as another app does. The app is not let read it.
    private func copyInAnotherApp(_ text: String) throws {
        XCTAssertEqual(try control("pbcopy:\(text)"), "done")
    }

    private func pasteboardText() throws -> String {
        try control("pbpaste")
    }

    /// The page's own copy command, run by a tap on its button; it says "copied" when the system took the text.
    private func copyWithThePage() {
        app.webViews.buttons["Copy the code"].tap()
        waitForPage("copied")
    }

    /// The labels of the menu items and the small buttons on the screen, and whether the menu has a next page, read
    /// from one snapshot of the screen so that a menu that goes away while it is read does not fail the read.
    private func menuSnapshot() -> (labels: [String], hasNextPage: Bool) {
        guard let root = try? app.snapshot() else { return ([], false) }
        var labels: [String] = []
        var hasNextPage = false
        func walk(_ element: XCUIElementSnapshot) {
            if element.elementType == .menuItem || (element.elementType == .button && element.frame.height < 80) {
                labels.append(element.label)
                if element.label == "Next Page" && element.isEnabled { hasNextPage = true }
            }
            element.children.forEach(walk)
        }
        walk(root)
        return (labels, hasNextPage)
    }

    private func menuLabels() -> [String] {
        menuSnapshot().labels
    }

    /// Waits until a menu is on the screen and says everything it offers, page after page.
    private func menuOffer(timeout: TimeInterval = 10) -> [String] {
        _ = app.buttons["Next Page"].waitForExistence(timeout: timeout)
        var offered: [String] = []
        for _ in 0 ..< 4 {
            let page = menuSnapshot()
            offered += page.labels
            guard page.hasNextPage else { break }
            app.buttons["Next Page"].firstMatch.tap()
            Thread.sleep(forTimeInterval: 0.7)
        }
        return offered
    }

    // MARK: The sign and the page out of screenshots

    /// The sign says what the page is kept out of once the page is in the secure canvas, and there is no line that
    /// asks the person to read anything. The Simulator draws the page as it always does, so this shows the page was
    /// put in the canvas and the state followed; whether a screenshot is black is for a real iPhone.
    func testTheSignSaysThePageIsHiddenFromScreenshotsAndNoLineAsksToReadAnything() {
        launch()
        openGlasshouse()
        XCTAssertTrue(sign().waitForExistence(timeout: 10), "no sign of restricted mode")
        wait("the sign to say the page is hidden") { self.sign().label == Message.restrictedAndHidden }
        go(site.trusted + "/restricted")
        waitForPage("Restricted notes")
        XCTAssertEqual(sign().label, Message.restrictedAndHidden, "the sign changed after the page opened")
        XCTAssertFalse(app.staticTexts[Message.screenshot].exists, "a line says a screenshot cannot be blocked")
        XCTAssertFalse(app.buttons["OK"].exists, "a line waits to be read")
    }

    // MARK: The page with its layer moved

    func testThePageScrollsAndTakesATapAtTheEnd() {
        launch()
        openGlasshouse()
        go(site.trusted + "/long")
        waitForPage("Top of the long page")
        let button = app.webViews.buttons["Tap at the end"]
        for _ in 0 ..< 20 where !button.isHittable {
            app.webViews.firstMatch.swipeUp()
        }
        XCTAssertTrue(button.isHittable, "the page did not scroll to its end")
        button.tap()
        waitForPage("tapped at the end")
    }

    func testThePageKeepsWorkingAfterThePersonLeavesItAndComesBack() {
        openRestrictedPage()
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        app.tabBars.buttons["Glasshouse"].tap()
        XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 10))
        wait("the sign to say the page is hidden") { self.sign().label == Message.restrictedAndHidden }
        waitForPage("Restricted notes")
        app.webViews.buttons["Select the words"].tap()
        page("Basil likes warmth").tap()
        let labels = menuOffer()
        XCTAssertTrue(labels.contains("Look Up"), "no menu on the selected text after coming back: \(labels)")
        XCTAssertFalse(app.staticTexts[Message.screenshot].exists)
    }

    // MARK: The touch menu

    func testTheTouchMenuOfSelectedTextOffersNoCopyCutOrShare() {
        openRestrictedPage()
        // The page selects its sentence (its own command); a touch on the selection brings up the menu.
        app.webViews.buttons["Select the words"].tap()
        page("Basil likes warmth").tap()
        let labels = menuOffer()
        XCTAssertTrue(labels.contains("Look Up"), "no menu on the selected text: \(labels)")
        for refused in refusedActions {
            XCTAssertFalse(labels.contains(refused), "the menu offers \(refused): \(labels)")
        }
    }

    func testTheTouchMenuOfATextFieldKeepsPasteAndOffersNoCopyCutOrShare() {
        openRestrictedPage(pasteboardText: beforeText)
        let field = app.webViews.textFields.firstMatch
        field.tap()
        field.typeText("basil")
        field.doubleTap()
        let labels = menuOffer()
        for refused in refusedActions {
            XCTAssertFalse(labels.contains(refused), "the menu offers \(refused): \(labels)")
        }
        XCTAssertTrue(labels.contains("Paste"), "no Paste: \(labels)")
    }

    func testATouchAndHoldOnAPictureShowsNoMenu() {
        openRestrictedPage()
        app.webViews.images.firstMatch.press(forDuration: 1.5)
        Thread.sleep(forTimeInterval: 1)
        let labels = menuLabels()
        for refused in refusedActions {
            XCTAssertFalse(labels.contains(refused), "the menu offers \(refused): \(labels)")
        }
        XCTAssertFalse(app.menuItems.firstMatch.exists, "a menu came up: \(labels)")
    }

    // MARK: The clipboard

    func testWhatThePageCopiesWorksInThePageAndIsGoneWhenTheAppGoesToTheBackground() throws {
        openRestrictedPage(pasteboardText: beforeText)
        copyWithThePage()
        // Copy and paste inside the page work: the touch menu of the field pastes what the page copied.
        let field = app.webViews.textFields.firstMatch
        field.tap()
        field.tap()
        let paste = app.descendants(matching: .any).matching(identifier: "Paste").firstMatch
        XCTAssertTrue(paste.waitForExistence(timeout: 10), "no Paste: \(menuLabels())")
        paste.tap()
        wait("the page's text to be pasted in the page") { (field.value as? String) == self.pageText }
        XCTAssertEqual(try pasteboardText(), pageText, "the page's own command did not copy")

        XCUIDevice.shared.press(.home)
        wait("the pasteboard to hold what was there before") { (try? self.pasteboardText()) == self.beforeText }
        app.activate()
        XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 10))
    }

    func testWhatThePageCopiesIsGoneWhenThePersonGoesToAnotherPageOfTheApp() throws {
        openRestrictedPage(pasteboardText: beforeText)
        copyWithThePage()
        XCTAssertEqual(try pasteboardText(), pageText, "the page's own command did not copy")
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        wait("the pasteboard to hold what was there before") { (try? self.pasteboardText()) == self.beforeText }
    }

    func testWhatThePageCopiesIsGoneWhenThePasteboardHeldNothingBefore() throws {
        openRestrictedPage(pasteboardText: "")
        XCTAssertEqual(try pasteboardText(), "")
        copyWithThePage()
        XCTAssertEqual(try pasteboardText(), pageText, "the page's own command did not copy")
        XCUIDevice.shared.press(.home)
        wait("the pasteboard to be empty again") { (try? self.pasteboardText()) == "" }
    }

    func testAPageThatCopiedNothingLeavesThePasteboardAsItIs() throws {
        openRestrictedPage(pasteboardText: beforeText)
        XCUIDevice.shared.press(.home)
        Thread.sleep(forTimeInterval: 2)
        XCTAssertEqual(try pasteboardText(), beforeText)
        app.activate()
        XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 10))
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        Thread.sleep(forTimeInterval: 1)
        XCTAssertEqual(try pasteboardText(), beforeText)
    }

    func testWhatAnotherAppCopiedWhileTheAppWasAwayIsLeftAlone() throws {
        openRestrictedPage(pasteboardText: beforeText)
        XCUIDevice.shared.press(.home)
        Thread.sleep(forTimeInterval: 2)
        try copyInAnotherApp("copied while away")
        app.activate()
        XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 10))
        Thread.sleep(forTimeInterval: 2)
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        Thread.sleep(forTimeInterval: 1)
        XCTAssertEqual(try pasteboardText(), "copied while away")
    }

    // MARK: Refused actions

    func testADownloadIsRefusedAndTold() {
        openRestrictedPage()
        app.webViews.links["Download the notes"].tap()
        XCTAssertTrue(
            app.staticTexts[Message.downloadBlocked].waitForExistence(timeout: 10), "no line about the download"
        )
        // The file the page asked to save was not asked for, and the page is the page it was.
        XCTAssertFalse(site.requests().contains { $0.path == "/notes.txt" }, "the file was fetched")
        XCTAssertTrue(page("Restricted notes").exists)
    }

    func testAFileTheServerSendsToSaveIsRefusedAndTold() {
        openRestrictedPage()
        app.webViews.links["Open the notes file"].tap()
        XCTAssertTrue(
            app.staticTexts[Message.downloadBlocked].waitForExistence(timeout: 10), "no line about the download"
        )
        XCTAssertTrue(page("Restricted notes").exists, "the page was replaced by the file")
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testAPrintIsRefusedAndTold() {
        openRestrictedPage()
        app.webViews.buttons["Print notes"].tap()
        XCTAssertTrue(app.staticTexts[Message.printBlocked].waitForExistence(timeout: 10), "no line about the print")
        XCTAssertFalse(app.sheets.firstMatch.exists, "a print panel came up")
    }

    func testAShareIsRefusedAndTold() {
        openRestrictedPage()
        app.webViews.buttons["Share notes"].tap()
        XCTAssertTrue(app.staticTexts[Message.shareBlocked].waitForExistence(timeout: 10), "no line about the share")
        waitForPage("share refused: NotAllowedError")
        XCTAssertFalse(app.otherElements["ActivityListView"].exists, "the share sheet came up")
    }

    func testTheLineOfARefusedActionGoesAwayAfterAFewSeconds() {
        openRestrictedPage()
        app.webViews.buttons["Print notes"].tap()
        let line = app.staticTexts[Message.printBlocked]
        XCTAssertTrue(line.waitForExistence(timeout: 10))
        XCTAssertTrue(line.waitForNonExistence(timeout: 20), "the line stays")
    }
}
