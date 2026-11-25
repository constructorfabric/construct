import Network
import XCTest

final class ComputerPageTests: ComputerTestCase {
    func testComputerPage() throws {
        let fixture = try fixturePort()
        continueAfterFailure = false
        app.launchArguments.append("--computer-page-test")
        app.launchEnvironment["CONSTRUCT_UI_TEST_ID"] = UUID().uuidString
        app.launch()

        app.tabBars.buttons["Memory"].tap()
        XCTAssertTrue(app.staticTexts["Alex Example"].waitForExistence(timeout: 10))
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(chat("Plan my week").waitForExistence(timeout: 10))
        openComputer(capturing: "settings")
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
        shot("not-paired")

        try pair(fixture)
        try verifyOutage(fixture)
        unpair()
        verifyCopyRemoved()

        app.terminate()
        app.launch()
        openComputer()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
        verifyCopyRemoved()
    }

    private func verifyCopyRemoved() {
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Memory"].tap()
        XCTAssertTrue(app.staticTexts["Alex Example"].waitForNonExistence(timeout: 10))
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.staticTexts["No chats yet"].waitForExistence(timeout: 10))
    }

    /// Another page tells that the computer is not reachable, with its own Try again, and nothing when it is.
    private func verifyNotice(reachable: Bool) {
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Memory"].tap()
        let unreachable = text(startingWith: "The computer is not reachable")
        if reachable {
            XCTAssertTrue(unreachable.waitForNonExistence(timeout: 30), "a notice on Memory while the computer answers")
        } else {
            XCTAssertTrue(unreachable.waitForExistence(timeout: 30), "no notice on Memory")
        }
        XCTAssertEqual(app.buttons.matching(identifier: "Try again").count, reachable ? 0 : 1)
        openComputer()
    }

    private func pair(_ fixture: NWEndpoint.Port) throws {
        try startPairing(fixture)
        XCTAssertTrue(app.staticTexts["Paired computer"].waitForExistence(timeout: 20))
        XCTAssertTrue(app.staticTexts["Connected"].waitForExistence(timeout: 20))
        shot("connected")
    }

    private func verifyOutage(_ fixture: NWEndpoint.Port) throws {
        _ = try exchange("stop", with: fixture)
        // A new start checks the computer at once, and the app then asks again only every two minutes.
        app.terminate()
        app.launch()
        openComputer()
        XCTAssertTrue(app.staticTexts["Not reachable"].waitForExistence(timeout: 30))
        let reason = NSPredicate(format: "label CONTAINS %@", "network change or router restart")
        XCTAssertTrue(app.staticTexts.matching(reason).firstMatch.exists)
        XCTAssertEqual(app.buttons.matching(identifier: "Try again").count, 1)
        shot("unreachable")
        // Pair again opens the choices to pair with another computer, and a second tap closes them.
        app.buttons["Pair again"].tap()
        XCTAssertTrue(app.buttons["Scan QR code"].exists)
        app.buttons["Pair again"].tap()
        XCTAssertTrue(app.buttons["Scan QR code"].waitForNonExistence(timeout: 5))
        verifyNotice(reachable: false)
        // The program runs again, but the app asks only every two minutes: it stays unreachable until Try again.
        _ = try exchange("restart", with: fixture)
        XCTAssertTrue(app.staticTexts["Not reachable"].exists)
        app.buttons["Try again"].tap()
        XCTAssertTrue(app.staticTexts["Connected"].waitForExistence(timeout: 20))
        verifyNotice(reachable: true)
    }

    private func unpair() {
        app.buttons["Unpair"].tap()
        let alert = app.alerts["Unpair this computer?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        shot("confirm-unpair")
        alert.buttons["Unpair"].tap()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
    }
}
