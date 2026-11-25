import ConstructShared
import Foundation

/// The words and the dates of the Dashboard page. The phone counts nothing: it words what the copy holds.
enum DashboardText {
    /// Whole minutes the way the computer's dashboard shows time: `45 min`, `1 h 5 min`.
    static func minutes(_ value: Int32) -> String {
        value < 60 ? "\(value) min" : "\(value / 60) h \(value % 60) min"
    }

    static func sessions(_ count: Int32) -> String {
        count == 1 ? "1 session" : "\(count) sessions"
    }

    static func hiddenProjects(_ count: Int32) -> String {
        count == 1
            ? "1 project is hidden on your computer."
            : "\(count) projects are hidden on your computer."
    }

    static let notSent = "Your computer has not sent the numbers for this day."
    static let notSentWeek = "Your computer has not sent the numbers for this week."
    static let emptyTitle = "Nothing from your computer yet"
    static let emptyText = "Pair this phone with your computer on the Settings page, and its dashboard shows here."
    static let noTable = "No time in this period."
    static let noSummary = "No time on this day."

    static func notSent(_ span: DashboardSpan) -> String {
        span == .week ? notSentWeek : notSent
    }

    static func title(_ kind: WidgetKind, groupBy: GroupBy?) -> String {
        switch kind {
        case .total: return "Time"
        case .barsPerDay: return "Time per day"
        case .daySummary: return "End of day"
        case .table: return "Time by \(groupName(groupBy))"
        }
    }

    private static func groupName(_ groupBy: GroupBy?) -> String {
        switch groupBy ?? .project {
        case .project: return "project"
        case .ticket: return "ticket"
        case .tool: return "tool"
        case .collection: return "group"
        case .day: return "day"
        case .app: return "app"
        case .site: return "website"
        }
    }

    static func sourceName(_ source: Source) -> String {
        switch source {
        case .aiSessions: return "AI sessions"
        case .onScreen: return "On screen"
        }
    }

    /// "Updated Nov 26, 2026 at 10:30", or the words for a copy that has no time.
    static func updated(_ saved: String?) -> String {
        guard let date = SyncText.date(saved) else { return "Not updated from your computer yet." }
        return "Updated \(date.formatted(date: .abbreviated, time: .shortened))"
    }
}

/// The days of the computer's calendar, in the computer's time zone, as dates of the phone's own types.
struct DashboardCalendar {
    let calendar: Calendar
    /// The words for months and weekdays and their order: the phone's own, so a person reads days as on their phone.
    private let locale: Locale

    /// A zone the phone does not know is the phone's own, as in the holder.
    init(zone: String?, locale: Locale = .current) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone.flatMap(TimeZone.init(identifier:)) ?? .current
        calendar.firstWeekday = 2
        self.calendar = calendar
        self.locale = locale
    }

    /// The middle of the day, so no change of the clock moves it to another date.
    func date(_ day: CalendarDay) -> Date {
        let parts = DateComponents(year: Int(day.year), month: Int(day.month), day: Int(day.day), hour: 12)
        return calendar.date(from: parts) ?? Date()
    }

    func day(of date: Date) -> CalendarDay {
        let parts = calendar.dateComponents([.year, .month, .day], from: date)
        return CalendarDay(year: Int32(parts.year ?? 1970), month: Int32(parts.month ?? 1), day: Int32(parts.day ?? 1))
    }

    /// A day from its label, `2026-11-16`.
    func day(label: String) -> CalendarDay? {
        let parts = label.split(separator: "-").compactMap { Int32($0) }
        return parts.count == 3 ? CalendarDay(year: parts[0], month: parts[1], day: parts[2]) : nil
    }

    private func style(_ format: Date.FormatStyle) -> Date.FormatStyle {
        var styled = format
        styled.calendar = calendar
        styled.locale = locale
        styled.timeZone = calendar.timeZone
        return styled
    }

    /// `Wed, Nov 18`.
    func text(_ day: CalendarDay) -> String {
        date(day).formatted(style(.dateTime.weekday(.abbreviated).month(.abbreviated).day()))
    }

    /// `Nov 16 – Nov 22`, or the one day when the first is the last.
    func range(_ first: CalendarDay, _ last: CalendarDay) -> String {
        guard first != last else { return text(first) }
        let short = style(.dateTime.month(.abbreviated).day())
        return "\(date(first).formatted(short)) – \(date(last).formatted(short))"
    }

    /// `Mon`, for the foot of a bar.
    func weekday(label: String) -> String {
        guard let day = day(label: label) else { return label }
        return date(day).formatted(style(.dateTime.weekday(.abbreviated)))
    }

    /// `Monday, Nov 16`, for a bar read out.
    func spoken(label: String) -> String {
        guard let day = day(label: label) else { return label }
        return date(day).formatted(style(.dateTime.weekday(.wide).month(.abbreviated).day()))
    }
}
