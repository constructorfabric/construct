import Network
import XCTest

/// The sync on the iPhone against the computer's test program, which serves made-up facts and chats from a file the
/// script changes while the test runs. The app runs with `--sync-test`: it checks every few seconds, and waits before
/// each step, so a test can see the loading screen, its counts and "Syncing".
final class SyncTests: ComputerTestCase {
    private let syncing = "Syncing with your computer."
    private let justSynced = "Up to date. Synced just now."
    private let unreachable = "The computer is not reachable"

    func testSync() throws {
        let fixture = try fixturePort()
        continueAfterFailure = false
        app.launchArguments.append("--sync-test")
        app.launchEnvironment["CONSTRUCT_UI_TEST_ID"] = UUID().uuidString
        app.launchEnvironment["CONSTRUCT_UI_TEST_EMPTY_COPY"] = "1"
        app.launch()

        openComputer()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
        try startPairing(fixture)
        try verifyFirstCopy()
        verifyPagesHoldTheComputersData()
        verifyTheSyncWhenTheAppOpens()
        try verifyChecksAndNewChat(fixture)
        try verifyNotReachable(fixture)
        verifyComputerPage()
    }

    /// A row of the loading screen: its name and its count are read out as one.
    private func row(_ name: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", "\(name),")).firstMatch
    }

    private func verifyFirstCopy() throws {
        XCTAssertTrue(app.staticTexts["Copying from your computer"].waitForExistence(timeout: 30))
        XCTAssertFalse(app.tabBars.buttons["Settings"].isHittable, "a touch reaches the tabs under the loading screen")
        XCTAssertFalse(
            app.navigationBars["Computer"].buttons.firstMatch.isHittable,
            "a touch reaches the page under the loading screen"
        )
        // The counts rise: the facts are copied first, then the chats one by one, then the dashboard.
        XCTAssertTrue(row("Memory").waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["Step 2 of 3"].waitForExistence(timeout: 30))
        XCTAssertTrue(row("Memory").label.contains("12 facts"), "Memory says \(row("Memory").label)")
        // The chats are under way, and the dashboard waits. A row under way says "copying", then the count of the list
        // as it is read ("4 chats"), then "k of 4": the test does not look for one of them.
        XCTAssertFalse(row("Chats").label.contains("waiting"), "Chats says \(row("Chats").label)")
        XCTAssertTrue(row("Dashboard").label.contains("waiting"), "Dashboard says \(row("Dashboard").label)")
        shot("loading")
        XCTAssertTrue(app.staticTexts["Step 3 of 3"].waitForExistence(timeout: 40))
        XCTAssertTrue(row("Chats").label.contains("4 chats"), "Chats says \(row("Chats").label)")
        XCTAssertTrue(app.staticTexts["Copying from your computer"].waitForNonExistence(timeout: 40))
        // The first copy is not followed by "Synced just now", and it leaves the page the person was on.
        XCTAssertTrue(app.staticTexts["Paired computer"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts[justSynced].exists)
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
    }

    private func verifyPagesHoldTheComputersData() {
        app.tabBars.buttons["Memory"].tap()
        XCTAssertTrue(text(startingWith: "Skill 1").waitForExistence(timeout: 10))
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(chat("Plan the trip").waitForExistence(timeout: 10))
        XCTAssertTrue(chat("Garden ideas").exists)
    }

    /// A sync when the app opens shows "Syncing", then "Synced just now" for a few seconds, then nothing.
    private func verifyTheSyncWhenTheAppOpens() {
        app.terminate()
        app.launch()
        XCTAssertTrue(app.staticTexts[syncing].waitForExistence(timeout: 15))
        shot("syncing")
        XCTAssertTrue(app.staticTexts[justSynced].waitForExistence(timeout: 15))
        shot("justsynced")
        XCTAssertTrue(app.staticTexts[justSynced].waitForNonExistence(timeout: 10), "the notice stays")
    }

    /// The check every few seconds shows nothing, and a chat the program gets appears in the open list.
    private func verifyChecksAndNewChat(_ fixture: NWEndpoint.Port) throws {
        // Three checks pass (the app asks every 6 seconds in this run) and none of them says anything.
        XCTAssertFalse(app.staticTexts[syncing].waitForExistence(timeout: 20), "a check showed \"Syncing\"")
        XCTAssertFalse(text(startingWith: unreachable).exists)
        XCTAssertFalse(chat("A chat from the computer").exists)
        _ = try exchange("chat", with: fixture)
        XCTAssertTrue(chat("A chat from the computer").waitForExistence(timeout: 30), "the open list did not change")
        XCTAssertFalse(app.staticTexts[syncing].exists)
    }

    /// Starts the app again with ten minutes between the checks, so that after the sync of the start no check is near.
    /// A step that taps while a check is about to end meets that check, not its own: a tap on "Sync now" or "Try again"
    /// joins it, so "Syncing…" is gone before the test looks for it, and a check that reaches the computer again clears
    /// the "not reachable" notice before the tap.
    private func restartWithFewChecks() {
        app.terminate()
        app.launchEnvironment["CONSTRUCT_SYNC_TEST_INTERVAL_MS"] = "600000"
        app.launch()
    }

    private func verifyComputerPage() {
        restartWithFewChecks()
        XCTAssertTrue(app.staticTexts[justSynced].waitForExistence(timeout: 20))
        XCTAssertTrue(app.staticTexts[justSynced].waitForNonExistence(timeout: 10))
        openComputer()
        XCTAssertTrue(text(startingWith: "Last sync ").waitForExistence(timeout: 10))
        let button = app.buttons["Sync now"]
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        shot("computer")
        button.tap()
        XCTAssertTrue(app.buttons["Syncing…"].waitForExistence(timeout: 10))
        XCTAssertTrue(button.waitForExistence(timeout: 20))
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
    }

    /// "Not reachable" shows once with "Try again", not on the Glasshouse page, and goes when a sync works.
    private func verifyNotReachable(_ fixture: NWEndpoint.Port) throws {
        app.tabBars.buttons["Chat"].tap()
        _ = try exchange("stop", with: fixture)
        let notice = text(startingWith: unreachable)
        XCTAssertTrue(notice.waitForExistence(timeout: 30))
        XCTAssertTrue(text(startingWith: "The computer is not reachable. Last sync").exists)
        XCTAssertEqual(app.buttons.matching(identifier: "Try again").count, 1)
        shot("unreachable")
        // The checks that follow do not tell it again: the notice is the same one, and "Syncing" never shows.
        XCTAssertFalse(app.staticTexts[syncing].waitForExistence(timeout: 14))
        XCTAssertTrue(notice.exists)
        app.tabBars.buttons["Glasshouse"].tap()
        XCTAssertFalse(text(startingWith: unreachable).exists, "a notice on the Glasshouse page")
        app.tabBars.buttons["Chat"].tap()
        // The computer stays off while the app is started again: the sync of the start misses it and shows the notice,
        // and no check can clear it before "Try again" does.
        restartWithFewChecks()
        XCTAssertTrue(notice.waitForExistence(timeout: 30), "no notice after the app was started again")
        _ = try exchange("restart", with: fixture)
        app.buttons["Try again"].tap()
        XCTAssertTrue(app.staticTexts[justSynced].waitForExistence(timeout: 30))
        XCTAssertFalse(text(startingWith: unreachable).exists)
        XCTAssertTrue(app.staticTexts[justSynced].waitForNonExistence(timeout: 10))
    }
}
