@testable import Construct
import WebKit
import XCTest

/// What restricted mode refuses on the Glasshouse page: touch menu actions, downloads, and the page's own print and
/// share.
@MainActor
final class GlasshouseRestrictionsTests: XCTestCase {
    private let address = URL(string: "https://example.test/file")!

    private func response(_ headers: [String: String] = [:]) -> HTTPURLResponse {
        HTTPURLResponse(url: address, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: headers)!
    }

    func testCopyCutAndShareAreRefusedInTheTouchMenuAndTheOtherActionsAreNot() {
        for name in ["copy:", "cut:", "share:", "_share:"] {
            XCTAssertFalse(GlasshouseRestrictions.allows(menuAction: NSSelectorFromString(name)), name)
        }
        for name in ["paste:", "selectAll:", "select:", "_lookup:", "_define:", "delete:"] {
            XCTAssertTrue(GlasshouseRestrictions.allows(menuAction: NSSelectorFromString(name)), name)
        }
    }

    func testAResponseTheWebViewCannotShowIsADownload() {
        XCTAssertTrue(GlasshouseRestrictions.isDownload(response(), canShow: false))
    }

    func testAResponseTheServerSendsAsAnAttachmentIsADownload() {
        for value in ["attachment", "attachment; filename=\"a.txt\"", "Attachment;filename=a.txt", "  ATTACHMENT"] {
            XCTAssertTrue(
                GlasshouseRestrictions.isDownload(response(["Content-Disposition": value]), canShow: true), value
            )
        }
    }

    func testAPageAndAnInlineFileAreNotDownloads() {
        XCTAssertFalse(GlasshouseRestrictions.isDownload(response(), canShow: true))
        for value in ["inline", "inline; filename=\"a.txt\""] {
            XCTAssertFalse(
                GlasshouseRestrictions.isDownload(response(["Content-Disposition": value]), canShow: true), value
            )
        }
        let other = URLResponse(url: address, mimeType: "text/html", expectedContentLength: 1, textEncodingName: nil)
        XCTAssertFalse(GlasshouseRestrictions.isDownload(other, canShow: true))
    }

    func testTheNamesOfTheBlockedActionsAreTheOnesTheScriptSends() {
        XCTAssertEqual(GlasshouseRestrictions.action(in: "print"), .print)
        XCTAssertEqual(GlasshouseRestrictions.action(in: "share"), .share)
        XCTAssertEqual(GlasshouseRestrictions.action(in: "download"), .download)
        XCTAssertNil(GlasshouseRestrictions.action(in: "format"))
        XCTAssertNil(GlasshouseRestrictions.action(in: 3))
    }

    // MARK: The page's script

    /// Loads a page with the script in a real web view and runs [code] in it, as the page's own script would.
    private func run(_ code: String, told: ((GlasshouseBlockedAction) -> Void)? = nil) async throws -> Any? {
        let configuration = WKWebViewConfiguration()
        configuration.userContentController.addUserScript(GlasshouseRestrictions.userScript())
        let handler = GlasshouseBlockedMessages { told?($0) }
        configuration.userContentController.add(handler, name: GlasshouseRestrictions.messageName)
        let view = WKWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 480), configuration: configuration)
        let loaded = LoadWaiter()
        view.navigationDelegate = loaded
        view.loadHTMLString("<html><body>page</body></html>", baseURL: URL(string: "https://example.test/"))
        try await loaded.finished()
        return try await view.callAsyncJavaScript(code, contentWorld: .page)
    }

    func testThePagesPrintCommandDoesNothingAndTellsTheApp() async throws {
        var told: [GlasshouseBlockedAction] = []
        _ = try await run("window.print();", told: { told.append($0) })
        try await Task.sleep(nanoseconds: 300_000_000)
        XCTAssertEqual(told, [.print])
    }

    func testThePagesShareCommandIsRejectedAsARefusalAndTellsTheApp() async throws {
        var told: [GlasshouseBlockedAction] = []
        let result = try await run(
            "try { await navigator.share({text: 'a'}); return 'shared'; } catch (error) { return error.name; }",
            told: { told.append($0) }
        )
        XCTAssertEqual(result as? String, "NotAllowedError")
        XCTAssertEqual(told, [.share])
    }

    func testThePageCannotShareAnything() async throws {
        let result = try await run("return navigator.canShare({text: 'a'});")
        XCTAssertEqual(result as? Bool, false)
    }
}

/// Waits for a web view to finish loading its page.
@MainActor
private final class LoadWaiter: NSObject, WKNavigationDelegate {
    private var continuation: CheckedContinuation<Void, Error>?

    func finished() async throws {
        try await withCheckedThrowingContinuation { continuation = $0 }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        continuation?.resume()
        continuation = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        continuation?.resume(throwing: error)
        continuation = nil
    }
}
