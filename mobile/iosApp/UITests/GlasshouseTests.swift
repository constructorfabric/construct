import XCTest

/// The Glasshouse page on an iPhone Simulator: each test types a real address into the real address bar and looks
/// at the real page.
final class GlasshouseTests: GlasshouseTestCase {
    // MARK: The address bar and the page

    func testATypedAddressOpensAndTheBarShowsItsHost() {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        XCTAssertTrue(page("Read the herb list").exists)
        // The bar shows the host of the page, not the whole address, and the keyboard has gone.
        XCTAssertEqual(app.buttons["glasshouse-host"].label, site.trustedHost)
        XCTAssertTrue(app.keyboards.element.waitForNonExistence(timeout: 5), "the keyboard is still there")
        XCTAssertFalse(app.textFields["glasshouse-address"].exists)
    }

    func testTheStartAddressOpensAtOnce() {
        launch(startAddress: site.trusted + "/")
        openGlasshouse()
        waitForPage("Garden notes")
    }

    func testNoStartAddressShowsTheNotice() {
        launch()
        openGlasshouse()
        XCTAssertTrue(app.staticTexts[Message.noStart].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["Reload"].exists, "the notice offers Reload")
        // The person types one, and the notice goes.
        go(site.trusted + "/")
        waitForPage("Garden notes")
        XCTAssertFalse(app.staticTexts[Message.noStart].exists)
    }

    func testTheServerSeesTheTokenOnThePageItsScriptAndItsPicture() throws {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        wait("the script and the picture to be fetched") {
            let paths = self.site.requests().map(\.path)
            return paths.contains("/garden.js") && paths.contains("/garden.png")
        }
        let seen = site.requests().filter { ["/", "/garden.js", "/garden.png"].contains($0.path) }
        XCTAssertEqual(Set(seen.map(\.path)), ["/", "/garden.js", "/garden.png"])
        for request in seen {
            // The web view's own User-Agent first, then the app's token, last, once.
            let sent = "\(request.path) sent \(request.userAgent)"
            XCTAssertTrue(request.userAgent.hasSuffix(" Construct/\(site.version)"), sent)
            XCTAssertEqual(request.userAgent.components(separatedBy: "Construct/").count, 2, request.userAgent)
            XCTAssertTrue(request.userAgent.contains("AppleWebKit/"), request.userAgent)
        }
    }

    func testASignInIsStillThereAfterTheAppRestarts() {
        launch()
        openGlasshouse()
        go(site.trusted + "/signin")
        waitForPage("Signed in")
        go(site.trusted + "/account")
        waitForPage("Session cookie: yes. Remember cookie: yes.")
        // As a person leaves an app: it goes to the background, and the system ends it.
        XCUIDevice.shared.press(.home)
        app.terminate()
        launch(forgetData: false)
        openGlasshouse()
        go(site.trusted + "/account")
        // The cookie that lasts a day is back. One with no end date is the page's to lose when the app ends, as in the
        // phone's own browser.
        waitForPage("Remember cookie: yes")
    }

    func testALinkThatAsksForANewWindowOpensInThePage() {
        launch()
        openGlasshouse()
        go(site.trusted + "/new-window")
        waitForPage("New windows")
        app.webViews.links["Open the herbs in a new window"].tap()
        waitForPage("Basil, mint and parsley.")
        app.buttons["glasshouse-back"].tap()
        waitForPage("New windows")
        app.webViews.buttons["Open the herbs from a script"].tap()
        waitForPage("Basil, mint and parsley.")
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testALinkToAnAppAddressDoesNothingAndLaunchesNothing() {
        launch()
        openGlasshouse()
        go(site.trusted + "/app-links")
        waitForPage("App links")
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for link in ["Link intent", "Link market", "Link tel", "Link mailto", "Link custom", "Link redirect"] {
            let element = app.webViews.links[link]
            XCTAssertTrue(element.waitForExistence(timeout: 10), "no \(link)")
            element.tap()
            Thread.sleep(forTimeInterval: 1.5)
            XCTAssertEqual(app.state, .runningForeground, "something else came up after \(link)")
            XCTAssertTrue(page("App links").exists, "the page changed after \(link)")
            XCTAssertFalse(app.staticTexts["glasshouse-message"].exists, "the page told of a problem after \(link)")
            XCTAssertEqual(springboard.alerts.count, 0, "a prompt came up after \(link)")
            XCTAssertEqual(app.buttons["glasshouse-host"].label, site.trustedHost)
        }
    }

    // MARK: Keeping the page

    func testThePageIsWholeAndWhatWasTypedInItIsStillThereAfterTheChatPage() {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        wait("the script and the picture to be fetched") {
            let paths = self.site.requests().map(\.path)
            return paths.contains("/garden.js") && paths.contains("/garden.png")
        }
        let note = app.webViews.textFields.firstMatch
        note.tap()
        note.typeText("basil")
        XCTAssertEqual(note.value as? String, "basil")
        let loads = site.requests().count
        // The keyboard covers the tab bar, so the person puts it away first, as on the phone.
        putTheKeyboardAway()
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        openGlasshouse()
        waitForPage("Garden notes")
        XCTAssertEqual(app.webViews.textFields.firstMatch.value as? String, "basil")
        // It is the same page, not one loaded again: the server was asked for nothing more.
        XCTAssertEqual(site.requests().count, loads)
    }

    func testAPageKeepsLoadingWhileThePersonIsOnTheChatPage() {
        launch()
        openGlasshouse()
        go(site.trusted + "/slow")
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        Thread.sleep(forTimeInterval: 6)
        openGlasshouse()
        waitForPage("Slow page loaded", timeout: 60)
    }

    // MARK: Back, forward and reload

    func testBackForwardAndReloadWork() {
        launch()
        openGlasshouse()
        XCTAssertFalse(app.buttons["glasshouse-back"].isEnabled)
        XCTAssertFalse(app.buttons["glasshouse-forward"].isEnabled)
        go(site.trusted + "/")
        waitForPage("Garden notes")
        app.webViews.links["Read the herb list"].tap()
        waitForPage("Basil, mint and parsley.")
        wait("back to be possible") { self.app.buttons["glasshouse-back"].isEnabled }
        app.buttons["glasshouse-back"].tap()
        waitForPage("Garden notes")
        wait("forward to be possible") { self.app.buttons["glasshouse-forward"].isEnabled }
        app.buttons["glasshouse-forward"].tap()
        waitForPage("Basil, mint and parsley.")

        let loads = site.requests().filter { $0.path == "/herbs" }.count
        app.buttons["glasshouse-reload"].tap()
        wait("the page to be fetched again") { self.site.requests().filter { $0.path == "/herbs" }.count > loads }
    }

    // MARK: Problems

    func testAnUntrustedCertificateShowsTheErrorAndReloadTriesAgain() {
        launch()
        openGlasshouse()
        go(site.untrusted + "/")
        waitForMessage(Message.notTrusted)
        XCTAssertFalse(app.webViews.staticTexts["Garden notes"].exists, "the page of the untrusted site shows")
        let before = site.connections("untrusted")
        app.buttons["Reload"].tap()
        wait("Reload to try the site again") { self.site.connections("untrusted") > before }
        waitForMessage(Message.notTrusted)
        let asked = site.requests().filter { $0.site == "untrusted" }
        XCTAssertEqual(asked.count, 0, "the untrusted site was asked for a page")
    }

    func testAnAddressThatCannotBeReachedShowsItsMessageAndReloadTriesAgain() {
        launch()
        openGlasshouse()
        // No site has this name.
        go("https://no-such-site.invalid/")
        waitForMessage(Message.notFound)
        XCTAssertTrue(app.buttons["Reload"].exists)

        // The site refuses until it listens; Reload then opens it.
        site.flaky(false)
        go(site.flaky + "/")
        waitForMessage(Message.refused)
        site.flaky(true)
        app.buttons["Reload"].tap()
        waitForPage("Garden notes")
        XCTAssertFalse(app.staticTexts["glasshouse-message"].exists)
    }

    func testAPhoneWithNoNetworkShowsItsMessageAndReloadTriesAgain() {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        post(TestSite.offlineNotification)
        Thread.sleep(forTimeInterval: 1)
        go(site.trusted + "/herbs")
        waitForMessage(Message.noNetwork)
        XCTAssertEqual(site.requests().filter { $0.path == "/herbs" }.count, 0, "a page was asked for with no network")
        // Reload with no network says the same; with the network back it opens the page.
        app.buttons["Reload"].tap()
        waitForMessage(Message.noNetwork)
        post(TestSite.onlineNotification)
        Thread.sleep(forTimeInterval: 1)
        app.buttons["Reload"].tap()
        waitForPage("Basil, mint and parsley.")
        XCTAssertFalse(app.staticTexts["glasshouse-message"].exists)
    }

    func testAPlainHttpAddressIsBlockedAsAPageThatCannotOpen() {
        launch()
        openGlasshouse()
        // A named site on plain http is refused by the phone before any request is made.
        go("http://plain.example.test/")
        waitForMessage(Message.other)
        XCTAssertTrue(app.buttons["Reload"].exists)
    }

    func testAnAddressThatIsNotAWebAddressIsRefusedAndTheOldPageStays() {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        go("ftp://example.test/herbs")
        XCTAssertTrue(app.staticTexts[Message.onlyWeb].waitForExistence(timeout: 5))
        // The refused address stays in the bar, ready to be edited, and the page is still there.
        XCTAssertEqual(app.textFields["glasshouse-address"].value as? String, "ftp://example.test/herbs")
        XCTAssertTrue(page("Garden notes").exists)
        app.buttons["glasshouse-cancel"].tap()
        XCTAssertFalse(app.staticTexts[Message.onlyWeb].exists)
        XCTAssertEqual(app.buttons["glasshouse-host"].label, site.trustedHost)
    }

    func testStoppingTheWebProcessLeavesTheAppRunningWithReload() throws {
        launch()
        openGlasshouse()
        go(site.trusted + "/")
        waitForPage("Garden notes")
        XCTAssertEqual(try control("crash"), "done")
        waitForMessage(Message.stopped)
        XCTAssertEqual(app.state, .runningForeground)
        // The rest of the app works.
        app.tabBars.buttons["Chat"].tap()
        XCTAssertTrue(app.navigationBars["Chat"].waitForExistence(timeout: 10))
        openGlasshouse()
        waitForMessage(Message.stopped)
        app.buttons["Reload"].tap()
        waitForPage("Garden notes")
        XCTAssertFalse(app.staticTexts["glasshouse-message"].exists)
    }

    // MARK: Permissions

    func testARequestForTheCameraOrTheLocationIsRefusedWithNoPrompt() {
        launch()
        openGlasshouse()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        go(site.trusted + "/permissions")
        waitForPage("camera:NotAllowedError", timeout: 30)
        waitForPage("location:code1", timeout: 30)
        XCTAssertEqual(app.alerts.count, 0, "a prompt came up")
        XCTAssertEqual(springboard.alerts.count, 0, "a prompt came up")
        XCTAssertEqual(app.state, .runningForeground)
    }
}
