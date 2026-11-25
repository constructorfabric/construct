import XCTest

final class MovingBetweenPagesTests: XCTestCase {
    private let titles = ["Chat", "Memory", "Dashboard", "Glasshouse", "Settings"]
    private let app = XCUIApplication()

    override func setUp() {
        continueAfterFailure = false
        app.launch()
    }

    func testTheTabBarHasTheFivePagesInOrder() {
        XCTAssertEqual(app.tabBars.buttons.allElementsBoundByIndex.map(\.label), titles)
    }

    func testEveryTabShowsItsPageAndTitle() {
        for title in titles {
            app.tabBars.buttons[title].tap()
            XCTAssertTrue(app.tabBars.buttons[title].isSelected, "\(title) is not selected")
            if title == "Glasshouse" {
                XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 5), "no address bar")
            } else {
                let bar = app.navigationBars[title]
                XCTAssertTrue(bar.waitForExistence(timeout: 5), "no navigation bar titled \(title)")
            }
        }
    }

    /// A phone that is not paired has no computer to sync with, so no page tells anything about one, and no page has a
    /// line for the connection: the Settings page only has its Computer row.
    func testNoPageTellsAboutTheComputerWhenThePhoneIsNotPaired() {
        for title in titles {
            app.tabBars.buttons[title].tap()
            let page = title == "Glasshouse" ? app.buttons["glasshouse-host"] : app.navigationBars[title]
            XCTAssertTrue(page.waitForExistence(timeout: 5), "\(title) did not open")
            XCTAssertFalse(app.staticTexts["Syncing with your computer."].exists, "a sync notice on \(title)")
            let unreachable = NSPredicate(format: "label BEGINSWITH %@", "The computer is not reachable")
            XCTAssertFalse(app.staticTexts.matching(unreachable).firstMatch.exists, "a notice on \(title)")
            if title != "Settings" {
                XCTAssertFalse(app.staticTexts["Not paired"].exists, "a connection line on \(title)")
            }
        }
    }

    func testMemoryShowsItsListAndAnotherPageDoesNot() {
        app.tabBars.buttons["Memory"].tap()
        XCTAssertTrue(app.staticTexts["Alex Example"].waitForExistence(timeout: 5), "Memory shows no fact")
        app.tabBars.buttons["Dashboard"].tap()
        XCTAssertTrue(app.navigationBars["Dashboard"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Alex Example"].exists, "Dashboard shows the list of Memory")
    }
}
