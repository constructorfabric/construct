@testable import Construct
import ConstructShared
import XCTest

@MainActor
final class ChatMessagesScreenTests: XCTestCase {
    func testMissingModelFailureBecomesVisibleWhenSavedSettingsAreReady() async {
        let failure = ChatReplyFailed(reason: ConstructKt.NO_CHAT_MODEL)

        XCTAssertNil(ChatMessagesScreen.visibleReply(failure, modelReady: false))
        let visible = ChatMessagesScreen.visibleReply(failure, modelReady: true) as? ChatReplyFailed
        XCTAssertEqual(visible?.reason, failure.reason)
    }
}
