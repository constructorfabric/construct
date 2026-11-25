import Network
import XCTest

/// The Dashboard page on the iPhone against the computer's test program, which serves its default dashboard (a total,
/// bars per day, a table by project, a table by ticket and the summary of a day) and made-up sessions on a few days.
/// The script that starts the program says what the page must show for them (`expect`), because the days are counted
/// from today. The app runs with `--sync-test`, with no check of the computer between the steps: only the sync of a
/// start, which the test asks for by starting the app again, changes the copy.
final class DashboardTests: ComputerTestCase {
    private var expected: [String: String] = [:]

    func testDashboard() throws {
        let fixture = try fixturePort()
        continueAfterFailure = false
        app.launchArguments.append("--sync-test")
        app.launchEnvironment["CONSTRUCT_UI_TEST_ID"] = UUID().uuidString
        app.launchEnvironment["CONSTRUCT_UI_TEST_EMPTY_COPY"] = "1"
        app.launchEnvironment["CONSTRUCT_SYNC_TEST_INTERVAL_MS"] = "600000"
        app.launch()

        verifyEmptyBeforeTheFirstCopy()
        try pairAndCopy(fixture)
        try verifyTodayWithEveryWidgetKind()
        verifyThePreviousDayAndWeek()
        verifyPickingADate()
        verifyADayAndAWeekTheComputerSentNothingFor()
        try verifyHiddenProjects(fixture)
        try verifyTheComputerOff(fixture)
        try verifyEmptyAfterUnpairing(fixture)
    }

    // MARK: Steps

    private func verifyEmptyBeforeTheFirstCopy() {
        openDashboard()
        let empty = element("dashboard-empty")
        XCTAssertTrue(empty.waitForExistence(timeout: 10), "no empty state")
        XCTAssertTrue(empty.label.hasPrefix("Nothing from your computer yet"), "the empty state says \(empty.label)")
        XCTAssertFalse(element("dashboard-widget-total").exists)
        shot("dashboard-empty")
    }

    private func pairAndCopy(_ fixture: NWEndpoint.Port) throws {
        openComputer()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
        try startPairing(fixture)
        XCTAssertTrue(app.staticTexts["Copying from your computer"].waitForExistence(timeout: 30))
        XCTAssertTrue(app.staticTexts["Copying from your computer"].waitForNonExistence(timeout: 120))
        XCTAssertTrue(app.staticTexts["Paired computer"].waitForExistence(timeout: 10))
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        expected = try parse(exchange("expect", with: fixture))
    }

    /// After the first copy the page draws every kind of widget of the document with the numbers of today, and
    /// says when they were brought up to date.
    private func verifyTodayWithEveryWidgetKind() throws {
        openDashboard()
        for widget in ["total", "per-day", "by-project", "by-ticket", "end-of-day"] {
            XCTAssertTrue(element("dashboard-widget-\(widget)").waitForExistence(timeout: 20), "no widget \(widget)")
        }
        XCTAssertFalse(element("dashboard-empty").exists)
        let updated = element("dashboard-updated")
        XCTAssertTrue(updated.label.hasPrefix("Updated "), "the time of the update says \(updated.label)")
        let minutes = try number("today.minutes")
        expectLabel(app.staticTexts["dashboard-value-total"], time(minutes), "the total of today")
        expectLabel(app.staticTexts["dashboard-sessions-total"], sessions(try number("today.sessions")), "the sessions")
        // Bars per day: a page of one day has one bar.
        let bar = element("dashboard-bar-per-day-\(try text("today"))")
        XCTAssertTrue(bar.exists, "no bar for today")
        XCTAssertTrue(bar.label.hasSuffix(time(minutes)), "the bar says \(bar.label)")
        XCTAssertEqual(elements(startingWith: "dashboard-bar-per-day-").count, 1)
        // Tables: one row for each project and each ticket of today.
        XCTAssertEqual(elements(startingWith: "dashboard-row-by-project-").count, 2)
        XCTAssertEqual(elements(startingWith: "dashboard-row-by-ticket-").count, 2)
        XCTAssertTrue(elements(startingWith: "dashboard-row-by-project-").contains { $0.label.contains("45 min") })
        XCTAssertTrue(elements(startingWith: "dashboard-row-by-project-").contains { $0.label.contains("20 min") })
        // The summary of the day: its total and a line for each project.
        expectLabel(app.staticTexts["dashboard-value-end-of-day"], time(minutes), "the summary of today")
        XCTAssertEqual(elements(startingWith: "dashboard-line-end-of-day-").count, 2)
        XCTAssertFalse(element("dashboard-not-sent").exists)
        XCTAssertFalse(element("dashboard-today").exists, "a Today button on today")
        XCTAssertFalse(element("dashboard-next").isEnabled, "the page can go into the future")
        shot("dashboard-today")
        app.swipeUp()
        shot("dashboard-today-lower")
        app.swipeDown()
    }

    /// Moving back by a day, and by a week, shows the numbers the copy holds for that day and week.
    private func verifyThePreviousDayAndWeek() {
        element("dashboard-previous").tap()
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("yesterday.minutes")), "yesterday")
        expectLabel(app.staticTexts["dashboard-sessions-total"], sessions(int("yesterday.sessions")), "yesterday")
        XCTAssertTrue(element("dashboard-today").exists, "no way back to today")
        XCTAssertTrue(element("dashboard-next").isEnabled)

        app.segmentedControls["dashboard-span"].buttons["Week"].tap()
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("yesterday.week.minutes")), "the week")
        XCTAssertEqual(element("dashboard-previous").label, "Previous week")
        XCTAssertEqual(elements(startingWith: "dashboard-bar-per-day-").count, 7, "a week has seven bars")
        // The summary of a day counts one day, whatever the page counts.
        expectLabel(app.staticTexts["dashboard-value-end-of-day"], time(int("yesterday.minutes")), "the summary")
        shot("dashboard-week")

        element("dashboard-previous").tap()
        if int("before.week.minutes") > 0 {
            expectLabel(app.staticTexts["dashboard-value-total"], time(int("before.week.minutes")), "the week before")
        } else {
            // When today is a Monday, yesterday is in the week before and that one before it holds nothing.
            XCTAssertTrue(element("dashboard-not-sent").waitForExistence(timeout: 20), "no line for a week with none")
        }
    }

    /// Picking a date shows the numbers of that day.
    private func verifyPickingADate() {
        app.segmentedControls["dashboard-span"].buttons["Day"].tap()
        element("dashboard-date").tap()
        let calendar = element("dashboard-calendar")
        XCTAssertTrue(calendar.waitForExistence(timeout: 10), "no calendar")
        shot("dashboard-calendar")
        tapDay(of: expected["picked"] ?? "")
        element("dashboard-calendar-done").tap()
        XCTAssertTrue(calendar.waitForNonExistence(timeout: 10))
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("picked.minutes")), "the day picked")
        expectLabel(app.staticTexts["dashboard-sessions-total"], sessions(int("picked.sessions")), "the day picked")
    }

    /// A day or a week the copy holds no numbers for says that the computer has not sent it, and shows no zeros.
    private func verifyADayAndAWeekTheComputerSentNothingFor() {
        app.segmentedControls["dashboard-span"].buttons["Week"].tap()
        for _ in 0..<3 { element("dashboard-previous").tap() }
        let notSent = element("dashboard-not-sent")
        XCTAssertTrue(notSent.waitForExistence(timeout: 10), "no line for a week with no numbers")
        XCTAssertTrue(notSent.label.hasPrefix("Your computer has not sent"), "the line says \(notSent.label)")
        XCTAssertFalse(app.staticTexts["dashboard-value-total"].exists, "numbers where nothing was sent")
        shot("dashboard-not-sent")
        app.segmentedControls["dashboard-span"].buttons["Day"].tap()
        XCTAssertTrue(notSent.waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["dashboard-value-total"].exists)
        element("dashboard-today").tap()
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("today.minutes")), "back to today")
    }

    /// The line of projects hidden on the computer shows when the day has time in one. The sync of a start brings the
    /// change of the computer.
    private func verifyHiddenProjects(_ fixture: NWEndpoint.Port) throws {
        XCTAssertFalse(element("dashboard-hidden-by-project").exists, "a line about hidden projects, with none hidden")
        _ = try exchange("hide", with: fixture)
        restart()
        openDashboard()
        let hidden = element("dashboard-hidden-by-project")
        XCTAssertTrue(hidden.waitForExistence(timeout: 90), "no line about the hidden project")
        XCTAssertEqual(hidden.label, "1 project is hidden on your computer.")
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("hidden.today.minutes")), "without the hidden")
        XCTAssertEqual(elements(startingWith: "dashboard-row-by-project-").count, 1)
        shot("dashboard-hidden")
        // A day with no time in the hidden project has no line.
        let before = element("dashboard-date").label
        let previous = element("dashboard-previous")
        previous.tap()
        if !dateMoved(from: before) {
            previous.tap()
            let again = dateMoved(from: before)
            XCTFail("the first tap on Previous did nothing; a second tap \(again ? "moved" : "did not move") the page")
        }
        expectLabel(app.staticTexts["dashboard-value-total"], time(int("yesterday.minutes")), "yesterday")
        XCTAssertFalse(element("dashboard-hidden-by-project").exists)
        element("dashboard-today").tap()
    }

    /// With the computer off the page keeps its numbers, and the notice says the computer is not reachable.
    private func verifyTheComputerOff(_ fixture: NWEndpoint.Port) throws {
        let total = app.staticTexts["dashboard-value-total"]
        expectLabel(total, time(int("hidden.today.minutes")), "before the computer stops")
        let updated = element("dashboard-updated").label
        _ = try exchange("stop", with: fixture)
        restart()
        openDashboard()
        XCTAssertTrue(text(startingWith: "The computer is not reachable").waitForExistence(timeout: 60), "no notice")
        expectLabel(total, time(int("hidden.today.minutes")), "with the computer off")
        XCTAssertEqual(element("dashboard-updated").label, updated, "the time of the update moved with no sync")
        XCTAssertTrue(element("dashboard-widget-by-ticket").exists)
        shot("dashboard-offline")
    }

    /// With nothing in the copy (after Unpair, which removes what came from the computer) the empty state shows.
    private func verifyEmptyAfterUnpairing(_ fixture: NWEndpoint.Port) throws {
        _ = try exchange("restart", with: fixture)
        app.buttons["Try again"].tap()
        XCTAssertTrue(app.staticTexts["Up to date. Synced just now."].waitForExistence(timeout: 60))
        openComputer()
        app.buttons["Unpair"].tap()
        let alert = app.alerts["Unpair this computer?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        alert.buttons["Unpair"].tap()
        XCTAssertTrue(app.staticTexts["Not paired yet"].waitForExistence(timeout: 10))
        app.navigationBars["Computer"].buttons.firstMatch.tap()
        openDashboard()
        XCTAssertTrue(element("dashboard-empty").waitForExistence(timeout: 10), "no empty state after unpairing")
        XCTAssertFalse(element("dashboard-widget-total").exists)
    }

    // MARK: Helpers

    private func restart() {
        app.terminate()
        app.launch()
    }

    private func openDashboard() {
        app.tabBars.buttons["Dashboard"].tap()
        XCTAssertTrue(app.navigationBars["Dashboard"].waitForExistence(timeout: 5))
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any)[identifier].firstMatch
    }

    private func elements(startingWith prefix: String) -> [XCUIElement] {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", prefix))
            .allElementsBoundByIndex
    }

    /// Whether the date of the page changes from `before` within a few seconds.
    private func dateMoved(from before: String) -> Bool {
        let moved = NSPredicate(format: "label != %@", before)
        let changed = XCTNSPredicateExpectation(predicate: moved, object: element("dashboard-date"))
        return XCTWaiter.wait(for: [changed], timeout: 6) == .completed
    }

    /// Waits for a text to say `label`: a page that was moved reads the copy again.
    private func expectLabel(_ item: XCUIElement, _ label: String, _ what: String) {
        XCTAssertTrue(item.waitForExistence(timeout: 20), "\(what): nothing on the page")
        let says = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", label), object: item)
        if XCTWaiter.wait(for: [says], timeout: 20) != .completed {
            XCTFail("\(what): the page says \(item.label), not \(label)")
        }
    }

    /// The page's words for whole minutes: `45 min`, `1 h 5 min`.
    private func time(_ minutes: Int) -> String {
        minutes < 60 ? "\(minutes) min" : "\(minutes / 60) h \(minutes % 60) min"
    }

    private func sessions(_ count: Int) -> String {
        count == 1 ? "1 session" : "\(count) sessions"
    }

    private func parse(_ reply: String) throws -> [String: String] {
        var facts: [String: String] = [:]
        for line in reply.split(separator: "\n") {
            let parts = line.split(separator: "=", maxSplits: 1).map(String.init)
            facts[try XCTUnwrap(parts.first)] = try XCTUnwrap(parts.last)
        }
        return facts
    }

    private func text(_ key: String) throws -> String {
        try XCTUnwrap(expected[key], "the script gave no \(key)")
    }

    private func number(_ key: String) throws -> Int {
        try XCTUnwrap(Int(text(key)))
    }

    private func int(_ key: String) -> Int {
        Int(expected[key] ?? "") ?? -1
    }

    /// Taps a day in the calendar of the date picker, going back through the months until it shows.
    private func tapDay(of label: String) {
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyy-MM-dd"
        guard let date = format.date(from: label) else { return XCTFail("no date in \(label)") }
        let parts = Calendar(identifier: .gregorian).dateComponents([.month, .day], from: date)
        format.dateFormat = "MMMM"
        let month = format.string(from: date)
        let day = parts.day ?? 0
        // The label of a day names its month and its number, in either order and with the weekday and year around it.
        let monthFirst = ".*\\b\(month) \(day)\\b.*"
        let dayFirst = ".*\\b\(day)\\b.*\\b\(month)\\b.*"
        let named = NSPredicate(format: "label MATCHES %@", "(\(monthFirst))|(\(dayFirst))")
        let days = element("dashboard-calendar").buttons.matching(named)
        for _ in 0..<3 where !days.firstMatch.exists {
            let back = app.buttons["Previous Month"]
            if back.exists { back.tap() }
        }
        XCTAssertTrue(days.firstMatch.waitForExistence(timeout: 5), "no day \(label) in the calendar")
        days.firstMatch.tap()
    }
}
