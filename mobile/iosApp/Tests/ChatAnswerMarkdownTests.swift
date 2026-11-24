@testable import Construct
import XCTest

final class ChatAnswerMarkdownTests: XCTestCase {
    func testOnlyWebLinksKeepTheirDestinations() {
        let markdown = "[Web](https://example.test) [Local](file:///example) "
            + "[Custom](example-app://open) [Relative](article.md) [HTTP](http://example.test)"
        let rendered = ChatAnswerMarkdown.inline(markdown)

        XCTAssertEqual(String(rendered.characters), "Web Local Custom Relative HTTP")
        XCTAssertEqual(rendered.runs.compactMap { $0.link?.scheme }, ["https", "http"])
    }
}
