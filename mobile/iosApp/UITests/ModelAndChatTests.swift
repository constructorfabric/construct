import XCTest

final class ModelAndChatTests: XCTestCase {
    private let app = XCUIApplication()
    private let key = "sk-fake-ui-0123456789-do-not-use"
    private let model = "demo-model-1"
    private let saved = "A key is saved. Leave this empty to keep it."
    private var provider: LocalModelProvider?
    private var capture: Bool { ProcessInfo.processInfo.environment["CAPTURE_MOBILE_STATES"] == "1" }

    override func setUp() {
        continueAfterFailure = false
        app.launchEnvironment["CONSTRUCT_UI_TEST_ID"] = UUID().uuidString
    }

    override func tearDown() {
        provider?.close()
    }

    func testModelSetupSavesTestsAndRemovesTheKey() throws {
        let address = try serve([
            .json(200, #"{"choices":[{"message":{"content":"pong"}}]}"#, pause: capture ? 4 : 0),
            .json(200, Self.toolCall),
            .json(200, #"{"choices":[{"message":{"content":"The sum is 5."}}]}"#)
        ])
        app.launch()
        settingsTab()
        shot("settings-nomodel")

        XCTAssertFalse(app.buttons["Test"].isEnabled)
        app.buttons["Save"].tap()
        assertText("This field is required")
        shot("settings-fielderror")
        setUpModel(address: address)
        shot("settings-saved")

        app.terminate()
        app.launch()
        settingsTab()
        assertText(saved)
        XCTAssertEqual(app.textFields["Address"].value as? String, address)
        XCTAssertEqual(app.textFields["Model name"].value as? String, model)
        XCTAssertFalse(app.staticTexts[key].exists)
        shot("settings-ready")
        app.buttons["Test"].tap()
        if capture {
            assertText("Answers: checking")
            shot("settings-testrunning")
        }
        assertText("Answers: works")
        assertText("Uses tools: works")
        shot("settings-testpassed")

        replace(app.textFields["Model name"], with: "missing-model")
        app.buttons["Test"].tap()
        assertText("Answers: the provider has no model with this name.")
        assertText("Uses tools: skipped")
        shot("settings-testfailed")

        app.buttons["Remove key"].tap()
        XCTAssertTrue(app.buttons["Remove key"].waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts[saved].exists)
        XCTAssertEqual(app.textFields["Address"].value as? String, address)
    }

    func testPhoneChatStreamsStopsRetriesAndDeletes() throws {
        let address = try serve([
            .stream("A partial answer."),
            .stream("A complete answer.", pause: 1),
            .stream("Another answer.", pause: 0)
        ])
        app.launch()
        XCTAssertTrue(chat("Plan my week").waitForExistence(timeout: 10))
        newChat()
        assertText("No model is set yet. Set one in Settings to chat.")
        shot("chat-nomodel")

        app.buttons["Open Settings"].tap()
        assertText("Model")
        setUpModel(address: address)
        app.tabBars.buttons["Chat"].tap()
        assertText("Model: \(model)")
        shot("chat-new")
        send("Stop this answer")
        assertText("A partial answer.")
        XCTAssertNotEqual(app.textFields["Message Construct"].value as? String, "Stop this answer")
        shot("chat-answering")
        app.buttons["Stop"].tap()
        assertText("Stopped. The part that was written is kept.")
        assertText("A partial answer.")
        shot("chat-stopped")

        app.buttons["Try again"].tap()
        assertText("A complete answer.")
        XCTAssertTrue(app.buttons["Stop"].waitForNonExistence(timeout: 10))
        shot("chat-thread")
        app.buttons["Conversation actions"].tap()
        app.buttons["Delete"].tap()
        app.alerts.buttons["Delete"].tap()
        XCTAssertTrue(chat("Plan my week").waitForExistence(timeout: 10))
        XCTAssertFalse(chat("Stop this answer").exists)

        newChat()
        send("Delete this too")
        assertText("Another answer.")
        XCTAssertTrue(app.buttons["Stop"].waitForNonExistence(timeout: 10))
        app.navigationBars.buttons["Chat"].tap()
        assertCompactList(newest: "Delete this too")
        shot("chat-list")
        app.buttons["Chat actions"].tap()
        app.buttons["Delete all"].tap()
        XCTAssertTrue(app.alerts["Delete all conversations?"].waitForExistence(timeout: 5))
        shot("chat-confirm")
        app.alerts.buttons["Yes, delete all"].tap()
        XCTAssertTrue(chat("Delete this too").waitForNonExistence(timeout: 10))
        XCTAssertTrue(chat("Plan my week").exists)
        settingsTab()
        app.buttons["Remove key"].tap()
        XCTAssertTrue(app.buttons["Remove key"].waitForNonExistence(timeout: 10))
    }

    func testChatShowsProviderReasonsAndKeepsComputerThreadsReadOnly() throws {
        let address = try serve([])
        app.launch()
        let computerChat = chat("What do you know about me?")
        XCTAssertTrue(computerChat.waitForExistence(timeout: 10))
        XCTAssertTrue(computerChat.label.contains("Desktop"))
        computerChat.tap()
        assertText("Looked up your profile")
        assertText("You are a mobile developer who prefers short answers.")
        assertText("This chat was started on a computer and is read-only.")
        XCTAssertFalse(app.buttons["Conversation actions"].exists)
        XCTAssertFalse(app.textFields["Message Construct"].exists)
        shot("chat-readonly")
        app.navigationBars.buttons["Chat"].tap()

        settingsTab()
        setUpModel(address: address, keyValue: "wrong-fake-key")
        app.tabBars.buttons["Chat"].tap()
        newChat()
        send("Check the key")
        assertText("The provider did not accept the API key.", timeout: 40)
        shot("chat-failed")

        app.buttons["Open Settings"].tap()
        assertText("Model")
        replace(app.textFields["Model name"], with: "missing-model")
        XCTAssertEqual(app.textFields["Model name"].value as? String, "missing-model")
        replace(app.secureTextFields["API key"], with: key)
        app.buttons["Save"].tap()
        assertText("Saved.")
        app.tabBars.buttons["Chat"].tap()
        assertText("Model: missing-model")
        app.buttons["Try again"].tap()
        assertText("The provider has no model with this name.", timeout: 40)
        XCTAssertEqual(app.scrollViews.staticTexts.matching(identifier: "Check the key").count, 1)
        shot("chat-model-not-found")
        app.buttons["Open Settings"].tap()
        app.buttons["Remove key"].tap()
        XCTAssertTrue(app.buttons["Remove key"].waitForNonExistence(timeout: 10))
    }

    func testEmptyCopyShowsTheNewChatAction() {
        app.launchEnvironment["CONSTRUCT_UI_TEST_EMPTY_COPY"] = "1"
        app.launch()
        assertText("No chats yet")
        shot("chat-empty")
        app.buttons["new-chat-empty-state"].tap()
        assertText("What can I help with?")
    }

    func testUnreachableProviderOffersSettingsAndChatReflectsKeyRemoval() throws {
        let address = try serve([])
        provider?.close()
        provider = nil
        app.launch()
        settingsTab()
        fillModel(address: address)
        app.buttons["Save"].tap()
        app.tabBars.buttons["Chat"].tap()
        newChat()
        assertText("Model: \(model)")
        send("Check the address")
        assertText("Nothing answered at the model address.", timeout: 40)
        app.buttons["Open Settings"].tap()
        assertText("Model")
        app.buttons["Remove key"].tap()
        app.tabBars.buttons["Chat"].tap()
        assertText("No model is set yet. Set one in Settings to chat.")
    }

    private func serve(_ replies: [LocalModelProvider.Reply]) throws -> String {
        let ready = expectation(description: "local model provider started")
        var port: UInt16 = 0
        provider = try LocalModelProvider(key: key, model: model, replies: replies) {
            port = $0
            ready.fulfill()
        }
        wait(for: [ready], timeout: 5)
        return "http://127.0.0.1:\(port)/v1"
    }

    private func settingsTab() {
        app.tabBars.buttons["Settings"].tap()
        assertText("Model")
    }

    private func setUpModel(address: String, keyValue: String? = nil) {
        fillModel(address: address, keyValue: keyValue)
        app.buttons["Save"].tap()
        assertText("Saved.")
        assertText(saved)
    }

    private func fillModel(address: String, keyValue: String? = nil) {
        replace(app.textFields["Address"], with: address)
        replace(app.textFields["Model name"], with: model)
        replace(app.secureTextFields["API key"], with: keyValue ?? key)
    }

    private func newChat() {
        app.navigationBars.buttons["New chat"].tap()
        assertText("What can I help with?")
    }

    private func send(_ text: String) {
        let composer = app.textFields["Message Construct"]
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        composer.tap()
        composer.typeText(text)
        app.buttons["Send"].tap()
    }

    private func chat(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    private func assertCompactList(newest title: String) {
        let phoneChat = chat(title)
        let desktopChat = chat("What do you know about me?")
        XCTAssertTrue(phoneChat.waitForExistence(timeout: 10))
        XCTAssertTrue(desktopChat.exists)
        XCTAssertTrue(phoneChat.label.contains("This phone"))
        XCTAssertTrue(desktopChat.label.contains("Desktop"))
        let rowSpacing = desktopChat.frame.minY - phoneChat.frame.minY
        XCTAssertGreaterThan(rowSpacing, 0)
        XCTAssertLessThan(rowSpacing, 80)
    }

    private func assertText(
        _ text: String,
        timeout: TimeInterval = 15,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertTrue(app.staticTexts[text].waitForExistence(timeout: timeout), text, file: file, line: line)
    }

    private func shot(_ name: String) {
        guard capture else { return }
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private static let toolCall = """
        {"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function",
        "function":{"name":"add","arguments":"{\\"a\\":2,\\"b\\":3}"}}]}}]}
        """
}

private extension ModelAndChatTests {
    func replace(_ field: XCUIElement, with text: String) {
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        let end = field.coordinate(withNormalizedOffset: CGVector(dx: 0.95, dy: 0.5))
        end.tap()
        for _ in 0..<3 {
            guard let value = field.value as? String, !value.isEmpty, value != field.placeholderValue else { break }
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: value.count))
            if let remaining = field.value as? String, !remaining.isEmpty, remaining != field.placeholderValue {
                end.tap()
            }
        }
        let value = field.value as? String
        XCTAssertTrue(value == "" || value == field.placeholderValue)
        field.typeText(text)
        let done = app.keyboards.buttons["Done"]
        if done.exists { done.tap() }
    }
}
