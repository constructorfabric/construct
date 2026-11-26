import Combine
import ConstructShared
import SwiftUI
import WebKit

/// Everything the Glasshouse page keeps while the app runs: the state holder, the one web view, and what carries
/// out the holder's commands on it. The shell makes it once and keeps it, so the page, what was typed in it and
/// its history stay while the person is on another page of the app, whatever SwiftUI rebuilds. The web view is
/// made when the page is first shown.
@MainActor
final class GlasshouseModel: NSObject, ObservableObject {
    let state: FlowState<GlasshouseState>
    @Published private(set) var webView: WKWebView?
    @Published private(set) var progress = 0.0
    /// The page's restricted mode, which the screen shows and the delegate calls tell what the page refused.
    let restriction: GlasshouseRestriction

    private let holder: GlasshouseHolder
    private let version: String
    private let reachability: GlasshouseReachability
    private var commands: Task<Void, Never>?
    private var observers: [AnyCancellable] = []
    /// The address of the page the web view is going to, as the main frame was last told to go.
    private var pending: URL?
    /// True from the moment a page fails before it shows until the next load: the page that was there is still the
    /// web view's current one, and "back" returns to it.
    private var failedBeforeShowing = false

    init(
        startAddress: String?,
        version: String,
        reachability: GlasshouseReachability = GlasshouseReachability(),
        restriction: GlasshouseRestriction? = nil
    ) {
        holder = GlasshouseHolder(configuredStartAddress: startAddress)
        state = FlowState(holder.state)
        self.version = version
        self.reachability = reachability
        self.restriction = restriction ?? GlasshouseRestriction()
        super.init()
        #if DEBUG && targetEnvironment(simulator)
        // A page test starts from no cookies and no site data, which are otherwise kept between launches.
        if ProcessInfo.processInfo.arguments.contains("--glasshouse-forget-data") {
            WKWebsiteDataStore.default().removeData(
                ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(),
                modifiedSince: .distantPast
            ) {}
        }
        // A restricted mode test needs something the app itself put on the pasteboard: the app may read that back, and
        // it cannot read what a test runner or another app put there. An empty text empties the pasteboard.
        if let text = ProcessInfo.processInfo.environment["CONSTRUCT_GLASSHOUSE_TEST_PASTEBOARD"] {
            UIPasteboard.general.items = text.isEmpty ? [] : [["public.utf8-plain-text": text]]
        }
        #endif
    }

    deinit {
        commands?.cancel()
    }

    var actions: GlasshouseActions {
        GlasshouseActions(
            openAddress: { [holder] text in
                holder.go(text: text)
                return !holder.state.value.addressRefused
            },
            edited: { [holder] in holder.addressEdited() },
            reload: { [holder] in holder.reload() },
            back: { [holder] in holder.back() },
            forward: { [holder] in holder.forward() },
            dismissScreenshotNotice: { [restriction] in restriction.dismissScreenshotNotice() },
            screenshotProtection: { [restriction] worked in restriction.screenshotProtection(worked: worked) },
            screenshotProtectionEnded: { [restriction] in restriction.screenshotProtectionEnded() }
        )
    }

    /// Makes the web view the first time the page is shown, and starts it on the start address.
    func startIfNeeded() {
        guard webView == nil else { return }
        let view = makeWebView()
        webView = view
        observe(view)
        // The holder is told once for each web view, before the commands are collected.
        holder.webViewStarted()
        commands = Task { [weak self] in
            guard let flow = self?.holder.commands else { return }
            for await command in flow {
                self?.run(command)
            }
        }
    }

    private func makeWebView() -> WKWebView {
        let configuration = WKWebViewConfiguration()
        // The app's own persistent data store: one set of cookies and site data for every page, kept after the
        // app restarts, and apart from the phone's own browser. It is the system's default for the app.
        configuration.websiteDataStore = .default()
        // The web view appends this to its own User-Agent, so `Construct/<version>` is last, on the page and on
        // everything it asks for, from the first request.
        configuration.applicationNameForUserAgent = "Construct/\(version)"
        configuration.userContentController.addUserScript(GlasshouseRefusals.userScript())
        // Restricted mode: a print or a share the page's script asks for is refused and said so.
        configuration.userContentController.addUserScript(GlasshouseRestrictions.userScript())
        configuration.userContentController.add(
            GlasshouseBlockedMessages { [weak self] action in self?.restriction.blocked(action) },
            name: GlasshouseRestrictions.messageName
        )
        let view = GlasshouseWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = self
        view.uiDelegate = self
        view.allowsBackForwardNavigationGestures = true
        view.allowsLinkPreview = false
        if #available(iOS 16.4, *) {
            // No developer tools: the system's Web Inspector is off for this web view.
            view.isInspectable = false
        }
        view.scrollView.contentInsetAdjustmentBehavior = .never
        view.underPageBackgroundColor = UIColor(ConstructTheme.color(.background))
        view.accessibilityIdentifier = "glasshouse-page"
        return view
    }

    private func observe(_ view: WKWebView) {
        observers = [
            view.publisher(for: \.estimatedProgress).sink { [weak self] value in self?.progress = value },
            Publishers.CombineLatest3(
                view.publisher(for: \.url),
                view.publisher(for: \.canGoBack),
                view.publisher(for: \.canGoForward)
            )
            .sink { [weak self] _, _, _ in self?.reportShown() }
        ]
    }

    // MARK: Commands

    private func run(_ command: GlasshouseCommand) {
        guard let view = webView else { return }
        switch onEnum(of: command) {
        case let .open(open): load(open.address)
        case .back: goBack(view)
        case .forward: view.goForward()
        case .reload: reload(view)
        }
    }

    private func load(_ address: String) {
        guard let view = webView else { return }
        guard let url = Self.url(from: address) else {
            holder.pageFailed(address: address, problem: .other)
            return
        }
        if reachability.isOffline {
            view.stopLoading()
            holder.pageFailed(address: address, problem: .offline)
            return
        }
        failedBeforeShowing = false
        pending = url
        view.load(URLRequest(url: url))
    }

    private func reload(_ view: WKWebView) {
        if view.url == nil {
            load(holder.state.value.address)
        } else if reachability.isOffline {
            view.stopLoading()
            holder.pageFailed(address: holder.state.value.address, problem: .offline)
        } else {
            failedBeforeShowing = false
            view.reload()
        }
    }

    private func goBack(_ view: WKWebView) {
        if failedBeforeShowing, view.url != nil {
            // The page that failed never replaced the one before it, so back is that page again.
            failedBeforeShowing = false
            view.reload()
        } else {
            view.goBack()
        }
    }

    /// The URL the system builds for [address], which keeps what the person typed. `URL(string:)` refuses an address
    /// that has a space or a letter outside ASCII in it (before iOS 17), so those are encoded for it.
    nonisolated static func url(from address: String) -> URL? {
        if let url = URL(string: address) { return url }
        let allowed = CharacterSet.urlHostAllowed
            .union(.urlPathAllowed)
            .union(.urlQueryAllowed)
            .union(.urlFragmentAllowed)
            .union(CharacterSet(charactersIn: "%:/?#[]@!$&'()*+,;="))
        return address.addingPercentEncoding(withAllowedCharacters: allowed).flatMap { URL(string: $0) }
    }

    // MARK: What the web view shows

    /// Tells the holder the address and the history the web view has now. A page that failed keeps the address it
    /// failed at until the next load.
    private func reportShown() {
        guard let view = webView, let url = view.url, !failedBeforeShowing else { return }
        holder.navigated(address: url.absoluteString, canGoBack: view.canGoBack, canGoForward: view.canGoForward)
    }

    private func failed(_ error: Error, shown: Bool) {
        let error = error as NSError
        guard let problem = GlasshouseProblems.problem(for: error, offline: reachability.isOffline) else {
            // The load was replaced or stopped by the app: nothing is wrong, and the page is what it was.
            if let view = webView, !view.isLoading {
                reportShown()
                holder.pageFinished()
            }
            return
        }
        failedBeforeShowing = !shown
        let address = (error.userInfo[NSURLErrorFailingURLStringErrorKey] as? String)
            ?? pending?.absoluteString
            ?? holder.state.value.address
        holder.pageFailed(address: address, problem: problem)
        if !shown, let view = webView {
            // The page before it is still there behind the failed one, so there is a page to go back to.
            holder.navigated(address: address, canGoBack: view.url != nil, canGoForward: false)
        }
    }
}

extension GlasshouseModel: WKNavigationDelegate {
    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        if navigationAction.shouldPerformDownload {
            restriction.blocked(.download)
            decisionHandler(.cancel)
            return
        }
        guard let url = navigationAction.request.url else {
            decisionHandler(.cancel)
            return
        }
        let frame: GlasshouseNavigation.Frame
        if let target = navigationAction.targetFrame {
            frame = target.isMainFrame ? .main : .inner
        } else {
            frame = .newWindow
        }
        switch GlasshouseNavigation.decide(url, frame: frame) {
        case .allow:
            if frame == .main { pending = url }
            decisionHandler(.allow)
        case .cancel:
            decisionHandler(.cancel)
        case .openHere:
            // A link or a script asks for a new window: it opens in this page.
            decisionHandler(.cancel)
            load(url.absoluteString)
        }
    }

    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationResponse: WKNavigationResponse,
        decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void
    ) {
        // A file to save, not a page to show, would be a download, which restricted mode refuses.
        if GlasshouseRestrictions.isDownload(navigationResponse.response, canShow: navigationResponse.canShowMIMEType) {
            restriction.blocked(.download)
            decisionHandler(.cancel)
        } else {
            decisionHandler(.allow)
        }
    }

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        failedBeforeShowing = false
        if let address = (pending ?? webView.url)?.absoluteString {
            holder.pageStarted(address: address)
        }
    }

    func webView(_ webView: WKWebView, didReceiveServerRedirectForProvisionalNavigation navigation: WKNavigation!) {
        if let address = pending?.absoluteString {
            holder.pageStarted(address: address)
        }
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        reportShown()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        reportShown()
        holder.pageFinished()
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        failed(error, shown: false)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        failed(error, shown: true)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        // The process of the page stopped; the app goes on. Reload makes the web view start a new one.
        holder.pageFailed(address: holder.state.value.address, problem: .crashed)
    }

    // The web view's own handling of a certificate it does not trust is kept: no `didReceive challenge` here.
}

extension GlasshouseModel: WKUIDelegate {
    /// A touch and hold on a link or a picture shows no menu: "Copy", "Share" and "Save image" are all in it.
    func webView(
        _ webView: WKWebView,
        contextMenuConfigurationFor elementInfo: WKContextMenuElementInfo,
        completionHandler: @escaping (UIContextMenuConfiguration?) -> Void
    ) {
        completionHandler(nil)
    }

    func webView(
        _ webView: WKWebView,
        createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction,
        windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        // A window the page asks for opens in this page, when it is a web address; no second web view is made.
        if let url = navigationAction.request.url, GlasshouseNavigation.decide(url, frame: .newWindow) == .openHere {
            load(url.absoluteString)
        }
        return nil
    }

    func webView(
        _ webView: WKWebView,
        requestMediaCapturePermissionFor origin: WKSecurityOrigin,
        initiatedByFrame frame: WKFrameInfo,
        type: WKMediaCaptureType,
        decisionHandler: @escaping (WKPermissionDecision) -> Void
    ) {
        decisionHandler(.deny)
    }

    func webView(
        _ webView: WKWebView,
        requestDeviceOrientationAndMotionPermissionFor origin: WKSecurityOrigin,
        initiatedByFrame frame: WKFrameInfo,
        decisionHandler: @escaping (WKPermissionDecision) -> Void
    ) {
        decisionHandler(.deny)
    }
}
