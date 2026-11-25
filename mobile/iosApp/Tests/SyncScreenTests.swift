@testable import Construct
import ConstructShared
import XCTest

/// What the loading screen of the first copy and the notices say about the sync, without drawing them.
final class SyncScreenTests: XCTestCase {
    private func progress(
        _ step: SyncStep,
        copied: Int32 = 0,
        total: Int32? = nil,
        done: Bool = false
    ) -> StepProgress {
        StepProgress(step: step, copied: copied, total: total.map { KotlinInt(int: $0) }, done: done)
    }

    func testTheBarIsAThirdForEachStepAndTheCopiedPartOfTheStepUnderWay() {
        XCTAssertEqual(FirstCopyScreen.fraction(SyncStep.allCases.map { progress($0) }), 0)
        let halfway = [
            progress(.memory, copied: 12, total: 12, done: true),
            progress(.chats, copied: 2, total: 4),
            progress(.dashboard)
        ]
        XCTAssertEqual(FirstCopyScreen.fraction(halfway), 0.5, accuracy: 0.0001)
        let counting = [progress(.memory, copied: 30), progress(.chats), progress(.dashboard)]
        XCTAssertEqual(FirstCopyScreen.fraction(counting), 0, "a step with no total yet adds nothing")
        let done = SyncStep.allCases.map { progress($0, copied: 1, total: 1, done: true) }
        XCTAssertEqual(FirstCopyScreen.fraction(done), 1)
        XCTAssertEqual(FirstCopyScreen.fraction([]), 0)
    }

    func testTheStepUnderWayIsTheFirstThatIsNotDone() {
        let steps = [progress(.memory, copied: 3, total: 3, done: true), progress(.chats), progress(.dashboard)]
        XCTAssertEqual(FirstCopyScreen.current(steps), 2)
        XCTAssertEqual(FirstCopyScreen.activeStep(steps), .chats)
        let done = SyncStep.allCases.map { progress($0, copied: 1, total: 1, done: true) }
        XCTAssertEqual(FirstCopyScreen.current(done), 3, "the last step stays the current one when all are done")
        XCTAssertNil(FirstCopyScreen.activeStep(done))
    }

    func testTheTimeOfTheLastSyncIsReadWithOrWithoutFractionalSeconds() throws {
        let whole = try XCTUnwrap(SyncText.date("2026-11-10T12:00:00Z"))
        XCTAssertEqual(whole.timeIntervalSince1970, 1_794_312_000)
        let micro = try XCTUnwrap(SyncText.date("2026-11-10T12:00:00.123456Z"))
        XCTAssertEqual(micro.timeIntervalSince(whole), 0.123456, accuracy: 0.001)
        let nano = try XCTUnwrap(SyncText.date("2026-11-10T12:00:00.123456789Z"))
        XCTAssertEqual(nano.timeIntervalSince(whole), 0.123456789, accuracy: 0.001)
        XCTAssertNil(SyncText.date(nil))
        XCTAssertNil(SyncText.date("yesterday"))
    }

    func testTheNoticeOfAComputerThatIsNotReachableGivesTheLastSyncWhenThereIsOne() throws {
        XCTAssertEqual(SyncText.notReachable(lastSynced: nil), "The computer is not reachable.")
        let text = SyncText.notReachable(lastSynced: try XCTUnwrap(SyncText.date("2026-11-10T12:00:00Z")))
        XCTAssertTrue(text.hasPrefix("The computer is not reachable. Last sync "), text)
        XCTAssertTrue(text.hasSuffix(" The copy on this phone is shown."), text)
    }

    func testEveryNoticeIconIsInTheAssetCatalog() {
        for name in ["NoticeCheck", "NoticeCircle", "NoticeMonitorOff"] {
            XCTAssertNotNil(UIImage(named: name), "no icon \(name)")
        }
    }
}
