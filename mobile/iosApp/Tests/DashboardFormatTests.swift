@testable import Construct
import ConstructShared
import XCTest

/// How the Dashboard page words time and days, without drawing it.
final class DashboardFormatTests: XCTestCase {
    func testTimeIsWholeMinutesAndThenHoursAndMinutes() {
        XCTAssertEqual(DashboardText.minutes(0), "0 min")
        XCTAssertEqual(DashboardText.minutes(45), "45 min")
        XCTAssertEqual(DashboardText.minutes(59), "59 min")
        XCTAssertEqual(DashboardText.minutes(60), "1 h 0 min")
        XCTAssertEqual(DashboardText.minutes(125), "2 h 5 min")
    }

    func testSessionsAndHiddenProjectsAreCounted() {
        XCTAssertEqual(DashboardText.sessions(1), "1 session")
        XCTAssertEqual(DashboardText.sessions(0), "0 sessions")
        XCTAssertEqual(DashboardText.sessions(3), "3 sessions")
        XCTAssertEqual(DashboardText.hiddenProjects(1), "1 project is hidden on your computer.")
        XCTAssertEqual(DashboardText.hiddenProjects(2), "2 projects are hidden on your computer.")
    }

    func testAWeekAndADayWithNoNumbersSayThatTheComputerHasNotSentThem() {
        XCTAssertEqual(DashboardText.notSent(.day), "Your computer has not sent the numbers for this day.")
        XCTAssertEqual(DashboardText.notSent(.week), "Your computer has not sent the numbers for this week.")
    }

    func testTheTitleOfAWidgetNamesWhatItCounts() {
        XCTAssertEqual(DashboardText.title(.total, groupBy: nil), "Time")
        XCTAssertEqual(DashboardText.title(.barsPerDay, groupBy: nil), "Time per day")
        XCTAssertEqual(DashboardText.title(.daySummary, groupBy: nil), "End of day")
        XCTAssertEqual(DashboardText.title(.table, groupBy: nil), "Time by project")
        XCTAssertEqual(DashboardText.title(.table, groupBy: .ticket), "Time by ticket")
        XCTAssertEqual(DashboardText.title(.table, groupBy: .site), "Time by website")
    }

    func testTheDaysAreTheDaysOfTheComputersZoneWhateverTheZoneOfThePhone() throws {
        let berlin = DashboardCalendar(zone: "Europe/Berlin")
        // 23:30 in Berlin on 18 November is still the 18th, and 22:30 UTC the same evening is 23:30 there.
        let evening = try XCTUnwrap(ISO8601DateFormatter().date(from: "2026-11-18T22:30:00Z"))
        XCTAssertEqual(berlin.day(of: evening), CalendarDay(year: 2026, month: 11, day: 18))
        let later = try XCTUnwrap(ISO8601DateFormatter().date(from: "2026-11-18T23:30:00Z"))
        XCTAssertEqual(berlin.day(of: later), CalendarDay(year: 2026, month: 11, day: 19))
        let auckland = DashboardCalendar(zone: "Pacific/Auckland")
        XCTAssertEqual(auckland.day(of: later), CalendarDay(year: 2026, month: 11, day: 19))
        XCTAssertEqual(berlin.day(of: berlin.date(CalendarDay(year: 2026, month: 3, day: 29))),
                       CalendarDay(year: 2026, month: 3, day: 29), "a day with a change of the clock")
    }

    func testARangeNamesItsFirstAndLastDayAndADayNamesItself() {
        let calendar = DashboardCalendar(zone: "Europe/Berlin", locale: Locale(identifier: "en_US"))
        let monday = CalendarDay(year: 2026, month: 11, day: 16)
        let sunday = CalendarDay(year: 2026, month: 11, day: 22)
        XCTAssertEqual(calendar.range(monday, sunday), "Nov 16 – Nov 22")
        XCTAssertEqual(calendar.range(monday, monday), "Mon, Nov 16")
        XCTAssertEqual(calendar.text(CalendarDay(year: 2026, month: 11, day: 18)), "Wed, Nov 18")
    }

    func testTheLabelOfADayGivesItsWeekday() {
        let calendar = DashboardCalendar(zone: "Europe/Berlin", locale: Locale(identifier: "en_US"))
        XCTAssertEqual(calendar.weekday(label: "2026-11-16"), "Mon")
        XCTAssertEqual(calendar.spoken(label: "2026-11-22"), "Sunday, Nov 22")
        XCTAssertEqual(calendar.day(label: "2026-11-16"), CalendarDay(year: 2026, month: 11, day: 16))
        XCTAssertNil(calendar.day(label: "yesterday"))
        XCTAssertEqual(calendar.weekday(label: "yesterday"), "yesterday")
    }

    func testDaysAreWordedAsOnThePhone() {
        let british = DashboardCalendar(zone: "Europe/Berlin", locale: Locale(identifier: "en_GB"))
        let monday = CalendarDay(year: 2026, month: 11, day: 16)
        let sunday = CalendarDay(year: 2026, month: 11, day: 22)
        XCTAssertEqual(british.range(monday, sunday), "16 Nov – 22 Nov")
        let german = DashboardCalendar(zone: "Europe/Berlin", locale: Locale(identifier: "de_DE"))
        XCTAssertTrue(german.weekday(label: "2026-11-16").hasPrefix("Mo"), "a Monday in German")
    }

    func testAZoneThePhoneDoesNotKnowIsThePhonesOwn() {
        XCTAssertEqual(DashboardCalendar(zone: "Nowhere/Land").calendar.timeZone, TimeZone.current)
        XCTAssertEqual(DashboardCalendar(zone: nil).calendar.timeZone, TimeZone.current)
    }

    func testTheTimeOfTheUpdateIsTheSyncsOrAnAdmission() {
        XCTAssertEqual(DashboardText.updated(nil), "Not updated from your computer yet.")
        XCTAssertTrue(DashboardText.updated("2026-11-10T12:00:00Z").hasPrefix("Updated "))
    }

    func testEveryWidgetKindAndGroupingHasAWord() {
        for kind in WidgetKind.allCases {
            XCTAssertFalse(DashboardText.title(kind, groupBy: .project).isEmpty)
        }
        for source in Source.allCases {
            XCTAssertFalse(DashboardText.sourceName(source).isEmpty)
        }
    }
}
