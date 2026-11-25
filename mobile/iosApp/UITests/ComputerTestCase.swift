import Network
import os
import XCTest

/// What a test that pairs the app with the computer's test program needs: the program's port on the loopback address,
/// the steps of pairing by typed address and code, and a screenshot kept for the report when a run asks for them.
class ComputerTestCase: XCTestCase {
    let app = XCUIApplication()
    private var capture: Bool { ProcessInfo.processInfo.environment["CAPTURE_MOBILE_STATES"] == "1" }

    func fixturePort() throws -> NWEndpoint.Port {
        guard let port = ProcessInfo.processInfo.environment["CONSTRUCT_PAIRING_READY_PORT"] else {
            throw XCTSkip("Run the script that starts the computer's test program.")
        }
        return try XCTUnwrap(NWEndpoint.Port(port))
    }

    func openComputer(capturing name: String? = nil) {
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        if let name { shot(name) }
        let row = app.descendants(matching: .any)["open-computer"].firstMatch
        for _ in 0..<3 where !row.isHittable { app.swipeUp() }
        XCTAssertTrue(row.isHittable)
        row.tap()
        XCTAssertTrue(app.navigationBars["Computer"].waitForExistence(timeout: 5))
    }

    func chat(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    /// A text that starts with `start`: a notice carries more than its first words.
    func text(startingWith start: String) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", start)).firstMatch
    }

    private func enter(_ text: String, in field: XCUIElement) {
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).tap()
        field.typeText(text)
    }

    /// Asks the program for an offer, types its address and code on the Computer page and taps Pair.
    func startPairing(_ fixture: NWEndpoint.Port) throws {
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
    }

    func exchange(_ command: String, with port: NWEndpoint.Port) throws -> String {
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

    func shot(_ name: String) {
        guard capture else { return }
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
