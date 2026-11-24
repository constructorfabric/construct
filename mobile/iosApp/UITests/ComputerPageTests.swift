import Network
import os
import XCTest

final class ComputerPageTests: XCTestCase {
    private let app = XCUIApplication()
    private var capture: Bool { ProcessInfo.processInfo.environment["CAPTURE_MOBILE_STATES"] == "1" }

    func testComputerPage() throws {
        guard let port = ProcessInfo.processInfo.environment["CONSTRUCT_PAIRING_READY_PORT"] else {
            throw XCTSkip("Run the Computer page fixture script.")
        }
        let fixture = try XCTUnwrap(NWEndpoint.Port(port))
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

    private func openComputer(capturing name: String? = nil) {
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        if let name { shot(name) }
        let row = app.descendants(matching: .any)["open-computer"].firstMatch
        for _ in 0..<3 where !row.isHittable { app.swipeUp() }
        XCTAssertTrue(row.isHittable)
        row.tap()
        XCTAssertTrue(app.navigationBars["Computer"].waitForExistence(timeout: 5))
    }

    /// The line under the title of another page shows the connection too, with its own Try again when it can help.
    private func verifyConnectionLine(_ title: String, retry: Bool) {
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Memory"].tap()
        XCTAssertTrue(app.staticTexts[title].waitForExistence(timeout: 30), "no \(title) line on Memory")
        XCTAssertEqual(app.buttons.matching(identifier: "Try again").count, retry ? 1 : 0)
        openComputer()
    }

    private func chat(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    private func enter(_ text: String, in field: XCUIElement) {
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).tap()
        field.typeText(text)
    }

    private func pair(_ fixture: NWEndpoint.Port) throws {
        let offer = try exchange("offer", with: fixture).split(separator: "\n")
        XCTAssertEqual(offer.count, 2, "The fixture must provide an address and code.")
        app.buttons["Scan QR code"].tap()
        XCTAssertTrue(app.staticTexts["The camera is unavailable."].waitForExistence(timeout: 10))
        app.buttons["camera-manual-entry"].tap()
        let address = app.textFields["computer-address"]
        let code = app.secureTextFields["computer-code"]
        XCTAssertTrue(address.waitForExistence(timeout: 5))
        shot("manual")
        enter(String(offer[0]), in: address)
        enter(String(offer[1]), in: code)
        app.buttons["Pair"].tap()
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
        let contact = NSPredicate(format: "label BEGINSWITH %@", "Last contact:")
        XCTAssertTrue(app.staticTexts.matching(reason).firstMatch.exists)
        XCTAssertTrue(app.staticTexts.matching(contact).firstMatch.exists)
        XCTAssertEqual(app.buttons.matching(identifier: "Try again").count, 1)
        shot("unreachable")
        // Pair again opens the choices to pair with another computer, and a second tap closes them.
        app.buttons["Pair again"].tap()
        XCTAssertTrue(app.buttons["Scan QR code"].exists)
        app.buttons["Pair again"].tap()
        XCTAssertTrue(app.buttons["Scan QR code"].waitForNonExistence(timeout: 5))
        verifyConnectionLine("The computer is not reachable", retry: true)
        // The program runs again, but the app asks only every two minutes: it stays unreachable until Try again.
        _ = try exchange("restart", with: fixture)
        XCTAssertTrue(app.staticTexts["Not reachable"].exists)
        app.buttons["Try again"].tap()
        XCTAssertTrue(app.staticTexts["Connected"].waitForExistence(timeout: 20))
        verifyConnectionLine("Connected", retry: false)
    }

    private func unpair() {
        app.buttons["Unpair"].tap()
        let alert = app.alerts["Unpair this computer?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        shot("confirm-unpair")
        alert.buttons["Unpair"].tap()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
    }

    private func exchange(_ command: String, with port: NWEndpoint.Port) throws -> String {
        // The offer goes directly to the test runner over loopback, without app launch arguments.
        let connection = NWConnection(host: "127.0.0.1", port: port, using: .tcp)
        defer { connection.cancel() }
        let received = XCTestExpectation(description: "The fixture answered \(command)")
        let reply = OSAllocatedUnfairLock<Data?>(initialState: nil)
        connection.start(queue: .global())
        connection.send(content: Data("\(command)\n".utf8), completion: .contentProcessed { _ in })
        connection.receive(minimumIncompleteLength: 1024, maximumLength: 1024) { data, _, complete, error in
            if complete && error == nil { reply.withLock { $0 = data } }
            received.fulfill()
        }
        XCTAssertEqual(XCTWaiter.wait(for: [received], timeout: 30), .completed)
        let data = try XCTUnwrap(reply.withLock { $0 })
        return try XCTUnwrap(String(bytes: data, encoding: .utf8))
    }

    private func shot(_ name: String) {
        guard capture else { return }
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
