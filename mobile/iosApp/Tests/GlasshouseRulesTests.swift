@testable import Construct
import ConstructShared
import XCTest

/// The small rules of the Glasshouse page that need no web view: how a failed load becomes a problem, what the address
/// bar shows, which address the system is given, and what the app's own configuration says.
final class GlasshouseRulesTests: XCTestCase {
    private func problem(_ code: URLError.Code, offline: Bool = false) -> GlasshouseProblem? {
        GlasshouseProblems.problem(for: NSError(domain: NSURLErrorDomain, code: code.rawValue), offline: offline)
    }

    func testANameThatDoesNotResolveIsNotFound() {
        XCTAssertEqual(problem(.cannotFindHost), .notFound)
        XCTAssertEqual(problem(.dnsLookupFailed), .notFound)
    }

    func testAConnectionThatFailsIsRefused() {
        XCTAssertEqual(problem(.cannotConnectToHost), .refused)
        XCTAssertEqual(problem(.networkConnectionLost), .refused)
    }

    func testACertificateTheSystemDoesNotTrustIsTheCertificateProblem() {
        for code in [URLError.Code.serverCertificateUntrusted, .serverCertificateHasBadDate,
                     .serverCertificateHasUnknownRoot, .serverCertificateNotYetValid] {
            XCTAssertEqual(problem(code), .certificate, "\(code.rawValue)")
        }
    }

    func testThePhonesOwnNoNetworkCodesAreOfflineAndTheOthersSayOfflineWhenThePhoneHasNone() {
        for code in [URLError.Code.notConnectedToInternet, .dataNotAllowed, .internationalRoamingOff, .callIsActive] {
            XCTAssertEqual(problem(code), .offline, "\(code.rawValue)")
        }
        for code in [URLError.Code.cannotFindHost, .cannotConnectToHost, .timedOut] {
            XCTAssertEqual(problem(code, offline: true), .offline, "\(code.rawValue)")
        }
    }

    func testAnythingElseIsOtherAndAPlainHttpPageTheSystemBlocksIsOther() {
        XCTAssertEqual(problem(.appTransportSecurityRequiresSecureConnection), .other)
        XCTAssertEqual(problem(.secureConnectionFailed), .other)
        XCTAssertEqual(problem(.timedOut), .other)
        XCTAssertEqual(GlasshouseProblems.problem(for: NSError(domain: "Elsewhere", code: 1), offline: false), .other)
    }

    func testALoadThatWasReplacedOrStoppedByTheAppIsNoProblem() {
        XCTAssertNil(problem(.cancelled))
        XCTAssertNil(GlasshouseProblems.problem(for: NSError(domain: "WebKitErrorDomain", code: 102), offline: false))
    }

    func testTheBarShowsTheHostWithItsPortAndNotTheWholeAddress() {
        XCTAssertEqual(GlasshouseHost.display("https://example.com/a?b#c"), "example.com")
        XCTAssertEqual(GlasshouseHost.display("https://user@example.com:8443/a"), "example.com:8443")
        XCTAssertEqual(GlasshouseHost.display("http://127.0.0.1:8080"), "127.0.0.1:8080")
        XCTAssertEqual(GlasshouseHost.display("ftp://example.com/a"), "ftp://example.com/a")
        XCTAssertEqual(GlasshouseHost.display(""), "")
    }

    func testTheSystemIsGivenTheAddressAsTypedAndAnAddressWithASpaceIsEncodedForIt() throws {
        let plain = "https://example.com/a?b=c#d"
        XCTAssertEqual(GlasshouseModel.url(from: plain)?.absoluteString, plain)
        let encoded = try XCTUnwrap(GlasshouseModel.url(from: "https://example.com/a b?c=d e"))
        XCTAssertEqual(encoded.host, "example.com")
        XCTAssertFalse(encoded.absoluteString.contains(" "))
        let escaped = "https://example.com/100%25"
        XCTAssertEqual(GlasshouseModel.url(from: escaped)?.absoluteString, escaped)
    }

    func testTheStartAddressComesFromTheBuildAndTheBuildHereSetsNone() throws {
        // The build here sets none: the key is in the app's Info.plist, empty, and the page says there is no address.
        XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: GlasshouseStartAddress.infoKey) as? String, "")
        XCTAssertNil(GlasshouseStartAddress.configured(environment: [:]))
        XCTAssertNil(GlasshouseAddress.shared.start(configured: GlasshouseStartAddress.configured(environment: [:])))
        // A build that sets one puts it under the Info.plist key, and the page reads it from there.
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let plist: [String: Any] = [GlasshouseStartAddress.infoKey: "https://start.example.test/"]
        try (plist as NSDictionary).write(to: folder.appendingPathComponent("Info.plist"))
        let bundle = try XCTUnwrap(Bundle(url: folder))
        let fromBuild = GlasshouseStartAddress.configured(environment: [:], bundle: bundle)
        XCTAssertEqual(fromBuild, "https://start.example.test/")
        // A debug build takes the address of its environment instead.
        let environment = ["CONSTRUCT_GLASSHOUSE_START_ADDRESS": "https://example.test"]
        let fromEnvironment = GlasshouseStartAddress.configured(environment: environment, bundle: bundle)
        XCTAssertEqual(fromEnvironment, "https://example.test")
    }

    func testTheUserAgentTokenIsTheVersionOfTheApp() {
        let version = GlasshouseStartAddress.appVersion()
        XCTAssertFalse(version.isEmpty)
        XCTAssertFalse(version.contains(" "))
        XCTAssertEqual(version, Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
    }

    /// The page asks for nothing the phone would have to be told about: the app has no text for the microphone, the
    /// location or the photos, and no exception to the phone's block on plain http. The camera text is the pairing
    /// page's, for the QR code; a page that asks for the camera is refused in code.
    func testTheAppAsksForNothingThePageCouldUse() {
        let info = Bundle.main.infoDictionary ?? [:]
        let keys = info.keys.filter { $0.hasPrefix("NS") && $0.hasSuffix("UsageDescription") }
        XCTAssertEqual(Set(keys), ["NSCameraUsageDescription", "NSLocalNetworkUsageDescription"])
        XCTAssertNil(info["NSAppTransportSecurity"])
    }

    func testTheIconsOfTheAddressBarAndTheToolbarAreInTheAssetCatalog() {
        for name in ["GlasshouseLock", "GlasshouseReload", "GlasshouseBack", "GlasshouseForward"] {
            XCTAssertNotNil(UIImage(named: name), "no icon \(name)")
        }
    }
}
