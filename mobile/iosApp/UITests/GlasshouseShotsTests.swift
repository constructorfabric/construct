import XCTest

/// A screenshot of each state of the Glasshouse page, for the pull request.
final class GlasshouseShotsTests: GlasshouseTestCase {
    /// Takes a screenshot of each state of the page, for the pull request. Only the screenshot run sets the
    /// variable; the runner sets the theme of the Simulator and names it in `GLASSHOUSE_THEME`.
    func testEveryStateForTheScreenshots() throws {
        let theme = try XCTUnwrap(ProcessInfo.processInfo.environment["GLASSHOUSE_THEME"], "Only the screenshot run.")
        launch()
        openGlasshouse()
        Thread.sleep(forTimeInterval: 1)
        shot("nostart", theme)

        go(site.trusted + "/")
        waitForPage("Garden notes")
        wait("the picture to be fetched") { self.site.requests().contains { $0.path == "/garden.png" } }
        Thread.sleep(forTimeInterval: 1)
        shot("ready", theme)

        go("ftp://example.test/herbs")
        _ = app.staticTexts[Message.onlyWeb].waitForExistence(timeout: 5)
        shot("refused", theme)
        app.buttons["glasshouse-cancel"].tap()

        app.buttons["glasshouse-host"].tap()
        _ = app.textFields["glasshouse-address"].waitForExistence(timeout: 5)
        Thread.sleep(forTimeInterval: 1)
        shot("typing", theme)
        app.buttons["glasshouse-cancel"].tap()

        go(site.trusted + "/slow")
        Thread.sleep(forTimeInterval: 1.5)
        shot("loading", theme)
        waitForPage("Slow page loaded", timeout: 60)

        go("https://no-such-site.invalid/")
        waitForMessage(Message.notFound)
        shot("notfound", theme)

        go(site.untrusted + "/")
        waitForMessage(Message.notTrusted)
        shot("certificate", theme)

        site.flaky(false)
        go(site.flaky + "/")
        waitForMessage(Message.refused)
        shot("connection-refused", theme)

        go(site.trusted + "/")
        waitForPage("Garden notes")
        XCTAssertEqual(try control("crash"), "done")
        waitForMessage(Message.stopped)
        shot("stopped", theme)

        go(site.trusted + "/")
        waitForPage("Garden notes")
        post(TestSite.offlineNotification)
        Thread.sleep(forTimeInterval: 1)
        go(site.trusted + "/herbs")
        waitForMessage(Message.noNetwork)
        shot("offline", theme)

        post(TestSite.onlineNotification)
        shootRestrictedMode(theme)
    }

    /// The page in restricted mode, and the line that tells a refused download.
    private func shootRestrictedMode(_ theme: String) {
        go(site.trusted + "/restricted")
        waitForPage("Restricted notes")
        Thread.sleep(forTimeInterval: 1)
        shot("restricted", theme)

        app.webViews.links["Download the notes"].tap()
        _ = app.staticTexts[Message.downloadBlocked].waitForExistence(timeout: 10)
        shot("blocked", theme)
    }
}
