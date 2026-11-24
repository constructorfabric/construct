@testable import Construct
import XCTest

/// The catalogue of pages. Their order is pinned by the UI test, which reads it from the real tab bar.
final class PageTests: XCTestCase {
    func testEveryPageHasItsOwnTitleAndItsOwnIcon() {
        XCTAssertEqual(Set(Page.allCases.map(\.title)).count, Page.allCases.count)
        XCTAssertEqual(Set(Page.allCases.map(\.icon)).count, Page.allCases.count)
    }

    func testEveryIconIsInTheAssetCatalog() {
        for page in Page.allCases {
            XCTAssertNotNil(UIImage(named: page.icon), "\(page.title) has no icon \(page.icon)")
        }
    }
}
