@testable import Construct
import ConstructShared
import XCTest

@MainActor
final class FlowStateTests: XCTestCase {
    private var scope = UiScope()

    override func setUp() {
        scope = UiScope()
    }

    override func tearDown() {
        scope.close()
    }

    func testViewSeesTheFactsOnceTheHolderHasRead() async throws {
        let holder = MemoryHolder(construct: FakeConstruct(scope: scope, deltaDelayMillis: 0), scope: scope)
        let state = FlowState(holder.state)

        try await wait { !state.value.loading }

        XCTAssertFalse(state.value.facts.isEmpty)
        XCTAssertNil(state.value.failure)
    }

    func testEveryLaterStateReachesTheView() async throws {
        let construct = FakeConstruct(scope: scope, deltaDelayMillis: 0)
        let holder = MemoryHolder(construct: construct, scope: scope)
        let state = FlowState(holder.state)
        try await wait { !state.value.loading }
        let facts = state.value.facts

        _ = try await construct.deleteMemoryFact(factId: facts[0].factId)
        holder.refresh()

        try await wait { state.value.facts.count == facts.count - 1 }
    }

    func testReleasingTheAdapterEndsIt() {
        let holder = MemoryHolder(construct: FakeConstruct(scope: scope, deltaDelayMillis: 0), scope: scope)
        var state: FlowState<MemoryState>? = FlowState(holder.state)
        weak var released = state

        state = nil

        XCTAssertNil(released, "the watching task kept the adapter alive")
        XCTAssertNil(state)
    }

    private func wait(for condition: @MainActor () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(5)
        while !condition() && Date() < deadline {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        XCTAssertTrue(condition())
    }
}
