import ConstructShared
import SwiftUI
import UIKit

/// What the sync tells a page: the notice, the time the copy was last brought up to date, and what "Try again" does.
struct SyncNoticeContent {
    let notice: SyncNotice
    let lastSynced: Date?
    let tryAgain: () -> Void
}

private struct SyncNoticeKey: EnvironmentKey {
    static let defaultValue: SyncNoticeContent? = nil
}

extension EnvironmentValues {
    /// Set by the frame for every page that has something to sync, and read by the first row of that page. A page
    /// without it (the Glasshouse page, a page one step down) shows no notice.
    var syncNotice: SyncNoticeContent? {
        get { self[SyncNoticeKey.self] }
        set { self[SyncNoticeKey.self] = newValue }
    }
}

enum SyncText {
    static let syncing = "Syncing with your computer."
    static let justSynced = "Up to date. Synced just now."

    static func notReachable(lastSynced: Date?) -> String {
        guard let lastSynced else { return "The computer is not reachable." }
        return "The computer is not reachable. Last sync \(lastSynced.formatted(date: .abbreviated, time: .shortened))."
            + " The copy on this phone is shown."
    }

    /// The time of the last sync as the Computer page words it.
    static func lastSync(_ date: Date) -> String {
        "Last sync \(date.formatted(date: .abbreviated, time: .shortened))"
    }

    /// The saved time (RFC 3339), or nil when the copy was never brought up to date.
    static func date(_ saved: String?) -> Date? {
        guard let saved else { return nil }
        return (try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(saved))
            ?? (try? Date.ISO8601FormatStyle().parse(saved))
    }
}

/// The notice at the top of a page: "Syncing with your computer.", "Up to date. Synced just now." for a few seconds,
/// and "The computer is not reachable" with "Try again". It is the first row of the page's list, so it scrolls away
/// with the page, and it draws nothing when the sync has nothing to tell.
struct SyncNoticeRow: View {
    @Environment(\.syncNotice) private var content

    var body: some View {
        if let content {
            switch content.notice {
            case .syncing:
                row(InlineNotice(text: SyncText.syncing, busy: true))
            case .justSynced:
                row(InlineNotice(text: SyncText.justSynced, icon: "NoticeCheck"))
            case .notReachable:
                row(InlineNotice(
                    text: SyncText.notReachable(lastSynced: content.lastSynced),
                    icon: "NoticeMonitorOff",
                    actions: [.init(title: "Try again", run: content.tryAgain)]
                ))
            default:
                EmptyView()
            }
        }
    }

    private func row(_ notice: InlineNotice) -> some View {
        notice
            .padding(.horizontal, ConstructTheme.Space.large)
            .padding(.vertical, ConstructTheme.Space.medium)
            .frame(maxWidth: .infinity, alignment: .leading)
            .listRowInsets(EdgeInsets())
            .listRowSeparator(.hidden)
            .listRowBackground(ConstructTheme.color(.background))
    }
}

/// A page that is not a list: the notice above the page's own content.
struct SyncNoticePage<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(spacing: ConstructTheme.Space.none) {
            SyncNoticeRow()
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

enum SyncAnnouncement {
    /// "Up to date. Synced just now." is read out once, after what VoiceOver is saying; the others are not.
    static func post(_ notice: SyncNotice) {
        guard notice == .justSynced else { return }
        UIAccessibility.post(notification: .announcement, argument: SyncText.justSynced)
    }
}
