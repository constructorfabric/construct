import Network
import notify
import os
import XCTest

/// What the page says, in the words of the desktop app.
enum Message {
    static let notTrusted = "The page cannot open: the site's certificate is not trusted."
    static let refused = "The page cannot open: the site refused the connection."
    static let notFound = "The page cannot open: no site was found at this address."
    static let noNetwork = "The page cannot open: there is no network connection."
    static let stopped = "The page stopped working."
    static let other = "The page cannot open."
    static let noStart = "No start address is set. Type an address above to open a page."
    static let onlyWeb = "Only http and https addresses open here."
    static let screenshot = "A screenshot of this page cannot be blocked on this iPhone."
    static let downloadBlocked = "Download is blocked in restricted mode."
    static let printBlocked = "Print is blocked in restricted mode."
    static let shareBlocked = "Share is blocked in restricted mode."
}

/// What the Glasshouse page tests share: the app, the test server of the Mac that runs them
/// (`scripts/test-glasshouse-ios.py`), and the ways to type an address and to look at the page. Run on their own,
/// without that script, the tests skip.
class GlasshouseTestCase: XCTestCase {
    let app = XCUIApplication()
    var site: TestSite!

    override func setUpWithError() throws {
        continueAfterFailure = false
        guard let found = TestSite.fromEnvironment() else { throw XCTSkip("Run the Glasshouse test script.") }
        site = found
        site.reset()
    }

    override func tearDown() {
        site?.flaky(false)
        post(TestSite.onlineNotification)
    }

    /// Starts the app. [pasteboardText] is put on the pasteboard by the app itself as it starts.
    func launch(startAddress: String? = nil, forgetData: Bool = true, pasteboardText: String? = nil) {
        app.launchArguments = forgetData ? ["--glasshouse-forget-data"] : []
        app.launchEnvironment["CONSTRUCT_GLASSHOUSE_START_ADDRESS"] = startAddress ?? ""
        if let pasteboardText {
            app.launchEnvironment["CONSTRUCT_GLASSHOUSE_TEST_PASTEBOARD"] = pasteboardText
        }
        app.launch()
    }

    func openGlasshouse() {
        app.tabBars.buttons["Glasshouse"].tap()
        XCTAssertTrue(app.buttons["glasshouse-host"].waitForExistence(timeout: 10), "no address bar")
    }

    // MARK: Helpers

    /// Types [address] into the address bar and goes. A bar that already has the field open (after a refusal) has its
    /// text replaced.
    func go(_ address: String) {
        let host = app.buttons["glasshouse-host"]
        if host.exists && host.isHittable { host.tap() }
        let field = app.textFields["glasshouse-address"]
        XCTAssertTrue(field.waitForExistence(timeout: 10), "the address field did not open")
        if let text = field.value as? String, !text.isEmpty, text != "Address" {
            field.coordinate(withNormalizedOffset: CGVector(dx: 0.97, dy: 0.5)).tap()
            // The first thing typed replaces the address when the field opens; here it is cleared by hand.
            if (field.value as? String) == text {
                field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: text.count))
            }
        }
        field.typeText(address + "\n")
    }

    func page(_ text: String) -> XCUIElement {
        app.webViews.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    func waitForPage(_ text: String, timeout: TimeInterval = 30) {
        if page(text).waitForExistence(timeout: timeout) { return }
        let shown = app.webViews.staticTexts.allElementsBoundByIndex.map(\.label)
        let bar = app.buttons["glasshouse-host"].label
        let message = app.staticTexts["glasshouse-message"]
        let told = message.exists ? message.label : "nothing"
        let asked = site.requests().map(\.path)
        XCTFail("the page does not show \"\(text)\", it shows \(shown); the bar says \(bar), the page tells \(told), "
            + "and the server was asked for \(asked)")
    }

    func waitForMessage(_ text: String) {
        XCTAssertTrue(app.staticTexts[text].waitForExistence(timeout: 60), "no message \"\(text)\"")
    }

    func wait(_ what: String, timeout: TimeInterval = 30, _ condition: @escaping () -> Bool) {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            XCTAssertTrue(Date() < deadline, "timed out waiting for \(what)")
            if Date() >= deadline { return }
            Thread.sleep(forTimeInterval: 0.25)
        }
    }

    /// Puts the keyboard away with the key of the phone's keyboard that does it.
    func putTheKeyboardAway() {
        let keys = [app.toolbars.buttons["Done"], app.keyboards.buttons["Done"], app.keyboards.buttons["return"],
                    app.keyboards.buttons["Return"], app.keyboards.buttons["Go"]]
        keys.first { $0.exists }?.tap()
        if app.keyboards.element.waitForNonExistence(timeout: 5) { return }
        let keyboard = app.keyboards.buttons.allElementsBoundByIndex.map(\.label)
        let bars = app.toolbars.buttons.allElementsBoundByIndex.map(\.label)
        XCTFail("the keyboard stays: its keys \(keyboard.suffix(8)), the buttons above it \(bars)")
    }

    func shot(_ state: String, _ theme: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "\(theme)-\(state)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// Tells the app (a debug Simulator build) that the phone has, or has lost, its network.
    func post(_ name: String) {
        notify_post(name)
    }

    /// Asks the runner, over loopback, for something only the Mac can do.
    func control(_ command: String) throws -> String {
        let port = try XCTUnwrap(NWEndpoint.Port(rawValue: site.controlPort))
        let connection = NWConnection(host: "127.0.0.1", port: port, using: .tcp)
        defer { connection.cancel() }
        let received = XCTestExpectation(description: "The runner answered \(command)")
        let reply = OSAllocatedUnfairLock<Data?>(initialState: nil)
        connection.start(queue: .global())
        connection.send(content: Data("\(command)\n".utf8), completion: .contentProcessed { _ in })
        connection.receive(minimumIncompleteLength: 1, maximumLength: 1024) { data, _, _, _ in
            reply.withLock { $0 = data }
            received.fulfill()
        }
        XCTAssertEqual(XCTWaiter.wait(for: [received], timeout: 30), .completed)
        let data = try XCTUnwrap(reply.withLock { $0 })
        return try XCTUnwrap(String(bytes: data, encoding: .utf8)).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// What the test server of the runner says about itself, and what it was asked for.
struct TestSite {
    struct Request: Decodable {
        let site: String
        let path: String
        let userAgent: String
    }

    static let offlineNotification = "construct.glasshouse.test.offline"
    static let onlineNotification = "construct.glasshouse.test.online"

    let trusted: String
    let untrusted: String
    let plain: String
    let flakyAddress: String
    let version: String
    let controlPort: UInt16

    var flaky: String { flakyAddress }

    /// What the address bar shows for a page of the trusted site: its host and port.
    var trustedHost: String { trusted.replacingOccurrences(of: "https://", with: "") }

    static func fromEnvironment() -> TestSite? {
        let env = ProcessInfo.processInfo.environment
        guard let trusted = env["GLASSHOUSE_TRUSTED"], let untrusted = env["GLASSHOUSE_UNTRUSTED"],
              let plain = env["GLASSHOUSE_PLAIN"], let flaky = env["GLASSHOUSE_FLAKY"],
              let version = env["GLASSHOUSE_VERSION"], let port = env["GLASSHOUSE_CONTROL_PORT"].flatMap(UInt16.init)
        else { return nil }
        return TestSite(
            trusted: trusted, untrusted: untrusted, plain: plain, flakyAddress: flaky,
            version: version, controlPort: port
        )
    }

    func requests() -> [Request] {
        (try? JSONDecoder().decode([Request].self, from: get("/__requests"))) ?? []
    }

    func connections(_ site: String) -> Int {
        let counts = (try? JSONSerialization.jsonObject(with: get("/__counts"))) as? [String: Int]
        return counts?[site] ?? 0
    }

    func reset() {
        _ = try? get("/__reset")
    }

    func flaky(_ listening: Bool) {
        _ = try? get("/__flaky?on=\(listening ? 1 : 0)")
    }

    /// The control calls go to the plain site, which asks nothing of certificates or of the phone's network rules.
    private func get(_ path: String) throws -> Data {
        guard let url = URL(string: plain + path) else { throw URLError(.badURL) }
        let done = DispatchSemaphore(value: 0)
        let box = OSAllocatedUnfairLock<Result<Data, Error>>(initialState: .failure(URLError(.timedOut)))
        URLSession.shared.dataTask(with: url) { data, _, error in
            box.withLock { $0 = data.map { .success($0) } ?? .failure(error ?? URLError(.unknown)) }
            done.signal()
        }.resume()
        _ = done.wait(timeout: .now() + 30)
        return try box.withLock { try $0.get() }
    }
}
