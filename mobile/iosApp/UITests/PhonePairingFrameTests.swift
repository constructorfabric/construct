import Network
import XCTest

final class PhonePairingFrameTests: XCTestCase {
    func testReconnectAfterAppRestart() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let portNumber = environment["CONSTRUCT_PAIRING_READY_PORT"] else {
            throw XCTSkip("Run the pairing fixture script.")
        }
        let port = try XCTUnwrap(NWEndpoint.Port(portNumber), "The fixture readiness port is invalid.")
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments.append("--pairing-frame-test")
        app.launch()
        defer { app.terminate() }

        XCTAssertTrue(app.staticTexts["The computer is not reachable"].waitForExistence(timeout: 20))
        let reason = NSPredicate(format: "label CONTAINS %@", "network change")
        let contact = NSPredicate(format: "label BEGINSWITH %@", "Last contact:")
        XCTAssertTrue(app.staticTexts.matching(reason).firstMatch.exists)
        XCTAssertTrue(app.staticTexts.matching(contact).firstMatch.exists)

        waitForListener(port)
        app.buttons["Try again"].tap()
        XCTAssertTrue(app.staticTexts["Connected"].waitForExistence(timeout: 10))
    }

    private func waitForListener(_ port: NWEndpoint.Port) {
        let signal = NWConnection(host: "127.0.0.1", port: port, using: .tcp)
        defer { signal.cancel() }
        let ready = XCTestExpectation(description: "The host has restarted the agent listener")
        signal.start(queue: .global())
        signal.receive(minimumIncompleteLength: 1, maximumLength: 1) { data, _, _, _ in
            if data == Data([1]) { ready.fulfill() }
        }
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 45), .completed)
    }
}
