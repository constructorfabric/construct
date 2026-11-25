@testable import Construct
import ConstructShared
import WebKit
import XCTest

/// The web view of the Glasshouse page, as set up: its data store, its User-Agent, and what it lets through.
@MainActor
final class GlasshouseModelTests: XCTestCase {
    private func started(version: String = "9.8.7") -> (GlasshouseModel, WKWebView) {
        let model = GlasshouseModel(startAddress: nil, version: version, reachability: GlasshouseReachability())
        model.startIfNeeded()
        return (model, model.webView!)
    }

    func testThereIsOneWebViewAndItIsMadeOnce() {
        let (model, view) = started()
        model.startIfNeeded()
        XCTAssertTrue(model.webView === view)
    }

    func testThePagesShareTheAppsOwnPersistentDataStore() {
        let (_, view) = started()
        XCTAssertTrue(view.configuration.websiteDataStore === WKWebsiteDataStore.default())
        XCTAssertTrue(view.configuration.websiteDataStore.isPersistent)
    }

    func testTheAppsTokenIsTheVersionAndItIsAddedToTheWebViewsOwnUserAgent() {
        let (_, view) = started(version: "9.8.7")
        XCTAssertEqual(view.configuration.applicationNameForUserAgent, "Construct/9.8.7")
        // Nothing replaces the web view's own User-Agent, so what it keeps (its engine and its device) stays.
        XCTAssertEqual(view.customUserAgent ?? "", "")
    }

    func testARequestForTheLocationIsRefusedByAScriptThatRunsFirstInEveryFrame() throws {
        let (_, view) = started()
        let scripts = view.configuration.userContentController.userScripts
        let refusal = try XCTUnwrap(scripts.first { $0.source.contains("geolocation") })
        XCTAssertEqual(refusal.injectionTime, .atDocumentStart)
        XCTAssertFalse(refusal.isForMainFrameOnly)
        XCTAssertEqual(scripts.count, 1)
    }

    func testAPageCannotAskForAnotherWindowByItselfAndTheWebViewHasNoInspector() {
        let (_, view) = started()
        XCTAssertFalse(view.configuration.preferences.javaScriptCanOpenWindowsAutomatically)
        if #available(iOS 16.4, *) {
            XCTAssertFalse(view.isInspectable)
        }
    }
}
