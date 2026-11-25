import Combine
import UIKit

/// What the Glasshouse screen draws of restricted mode, apart from the sign that is always there.
struct GlasshouseRestrictedState: Equatable {
    /// The action refused last, for a few seconds after it was refused.
    var blocked: GlasshouseBlockedAction?
    /// The page is hidden, because the screen is being recorded or shown on another screen.
    var covered = false
    /// The line that says a screenshot of the page cannot be blocked on this phone, until the person has read it.
    var screenshotNotice = false
}

/// The page's restricted mode while the app runs, kept by `GlasshouseModel`: the page is always in it and nothing
/// switches it off. It guards the clipboard while the page is shown, hides the page while the screen is recorded or
/// shown on another screen, and keeps the one line that tells a refused action.
@MainActor
final class GlasshouseRestriction: ObservableObject {
    @Published private(set) var state: GlasshouseRestrictedState

    private let clipboard: GlasshouseClipboard
    private let isCaptured: @MainActor () -> Bool
    private let noticeSeconds: Double
    private var visible = false
    private var appActive: Bool
    private var clearing: Task<Void, Never>?
    private var observers: [AnyCancellable] = []

    init(
        screenshotCanBeBlocked: Bool = GlasshouseScreenshot.canBeBlocked,
        clipboard: GlasshouseClipboard? = nil,
        center: NotificationCenter = .default,
        appActive: Bool? = nil,
        isCaptured: @escaping @MainActor () -> Bool = GlasshouseScreenshot.screenIsCaptured,
        noticeSeconds: Double = 6
    ) {
        state = GlasshouseRestrictedState(screenshotNotice: !screenshotCanBeBlocked)
        self.clipboard = clipboard ?? GlasshouseClipboard()
        self.appActive = appActive ?? (UIApplication.shared.applicationState == .active)
        self.isCaptured = isCaptured
        self.noticeSeconds = noticeSeconds
        observers = [
            center.publisher(for: UIApplication.willResignActiveNotification).sink { [weak self] _ in
                self?.appActive = false
                self?.refresh()
            },
            center.publisher(for: UIApplication.didBecomeActiveNotification).sink { [weak self] _ in
                self?.appActive = true
                self?.refresh()
            },
            center.publisher(for: UIScreen.capturedDidChangeNotification).sink { [weak self] _ in
                self?.refresh()
            },
            center.publisher(for: UIPasteboard.changedNotification).sink { [weak self] _ in
                self?.clipboard.pasteboardChanged()
            }
        ]
    }

    deinit {
        clearing?.cancel()
    }

    /// The page came onto the screen.
    func pageAppeared() {
        visible = true
        refresh()
    }

    /// The page left the screen: the person went to another page of the app.
    func pageDisappeared() {
        visible = false
        refresh()
    }

    /// The page refused [action]: the one line tells it for a few seconds.
    func blocked(_ action: GlasshouseBlockedAction) {
        state.blocked = action
        clearing?.cancel()
        let seconds = noticeSeconds
        clearing = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.state.blocked = nil
        }
    }

    func dismissScreenshotNotice() {
        state.screenshotNotice = false
    }

    private func refresh() {
        if visible && appActive {
            clipboard.pageIsShown()
        } else {
            clipboard.pageIsLeft()
        }
        let covered = visible && isCaptured()
        if state.covered != covered {
            state.covered = covered
        }
    }
}

/// What this phone can do about a screenshot or a recording of the page.
enum GlasshouseScreenshot {
    /// Whether the page can be kept out of a screenshot. Apple offers no way to do that. The one known way, a view in a
    /// secure text field, is not an Apple function and is reported to fail on iOS 18 and later, so the page does not
    /// rely on it: it says that a screenshot cannot be blocked.
    static let canBeBlocked = false

    /// Whether the screen is being recorded or shown on another screen, which Apple supports telling an app.
    @MainActor
    static func screenIsCaptured() -> Bool {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .contains { $0.screen.isCaptured }
    }
}
