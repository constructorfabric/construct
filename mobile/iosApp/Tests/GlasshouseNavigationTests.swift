@testable import Construct
import XCTest

/// Which navigations the page lets through.
final class GlasshouseNavigationTests: XCTestCase {
    private func decide(_ address: String, _ frame: GlasshouseNavigation.Frame) -> GlasshouseNavigation.Decision {
        GlasshouseNavigation.decide(URL(string: address)!, frame: frame)
    }

    func testAWebAddressIsAllowedAndAnyOtherAddressIsCancelledInTheMainFrame() {
        for address in ["https://example.test/a", "http://example.test/a", "HTTPS://EXAMPLE.TEST"] {
            XCTAssertEqual(decide(address, .main), .allow, address)
        }
        for address in ["tel:+15550100", "mailto:someone@example.test", "market://details?id=x", "gardenapp://open",
                        "intent://scan/#Intent;scheme=zxing;end", "sms:+15550100", "facetime:someone@example.test",
                        "itms-apps://apps.apple.com/app/id1", "maps://?q=garden", "file:///etc/hosts",
                        "data:text/html,hi", "about:blank", "blob:https://example.test/x", "ftp://example.test/a"] {
            XCTAssertEqual(decide(address, .main), .cancel, address)
        }
    }

    func testAnEmptyFrameInsideThePageIsAllowedAndNothingElseThatIsNotWeb() {
        XCTAssertEqual(decide("about:blank", .inner), .allow)
        XCTAssertEqual(decide("https://example.test/embed", .inner), .allow)
        XCTAssertEqual(decide("tel:+15550100", .inner), .cancel)
        XCTAssertEqual(decide("data:text/html,hi", .inner), .cancel)
    }

    func testAWebAddressInANewWindowOpensInThePageAndAnyOtherAddressDoesNothing() {
        XCTAssertEqual(decide("https://example.test/herbs", .newWindow), .openHere)
        XCTAssertEqual(decide("http://example.test/herbs", .newWindow), .openHere)
        for address in ["tel:+15550100", "about:blank", "market://details?id=x", "javascript:void(0)"] {
            XCTAssertEqual(decide(address, .newWindow), .cancel, address)
        }
    }
}
