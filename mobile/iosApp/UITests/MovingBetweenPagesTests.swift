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
            XCTAssertTrue(app.navigationBars[title].waitForExistence(timeout: 5), "no navigation bar titled \(title)")
        }
    }

    func testTheConnectionStateShowsOnEveryPage() {
        for title in titles {
            app.tabBars.buttons[title].tap()
            XCTAssertTrue(app.staticTexts["Not paired"].waitForExistence(timeout: 5), "no connection line on \(title)")
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
