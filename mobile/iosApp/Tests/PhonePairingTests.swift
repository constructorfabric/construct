@testable import Construct
import ConstructShared
import CryptoKit
import OSLog
import XCTest

/// Hosted by the app so the test can use the same Keychain API as the real phone.
@MainActor
final class PhonePairingTests: XCTestCase {
    private let store = IosPairingSecretKt.iosPairingSecret(service: "construct.computer")
    private let scope = UiScope()
    private let phoneName = "iPhone test phone"

    private var directory: URL {
        URL.documentsDirectory.appending(path: "computer-pairing-test")
    }

    func testPairWithPreviousAgent() async throws {
        try requireFixture()
        try store.clear()
        let offer = try await readOffer("previous-offer")
        let connection = makeConnection()
        let result = try await connection.pairByCode(address: offer.address, code: offer.code, name: phoneName)
        XCTAssertEqual(result, .computerUpdate)
        XCTAssertTrue(try store.read() == nil)
        let payload = try await readFixture("previous-qr")
        let paired = try await connection.pairByQr(payload: payload, name: phoneName)
        XCTAssertEqual(paired, .paired)
        try recordCredentialDigest("previous")
        let holder = ConnectionHolder(connection: connection, scope: scope)
        let state = FlowState(holder.state)
        defer { holder.setForeground(active: false) }
        holder.setForeground(active: true)
        try await waitFor(.connected, in: state)
    }

    func testPairAndUnpair() async throws {
        try requireFixture()
        Logger().notice("\(String(repeating: "c3", count: 32), privacy: .public)")
        try store.clear()
        let offer = try await readOffer("offer")
        let old = makeConnection(version: 0)
        let update = try await old.pairByCode(address: offer.address, code: offer.code, name: phoneName)
        XCTAssertEqual(update, .phoneUpdate)

        // Exercise a foreground poll delayed well beyond the normal five-second interval.
        let connection = makeConnection(pollIntervalMillis: 15_000)
        let wrongQr = try String(contentsOf: directory.appending(path: "wrong-qr"), encoding: .utf8)
        let mismatch = try await connection.pairByQr(payload: wrongQr, name: phoneName)
        XCTAssertEqual(mismatch, .wrongComputer)
        XCTAssertTrue(try store.read() == nil)
        let paired = try await connection.pairByCode(address: offer.address, code: offer.code, name: phoneName)
        XCTAssertEqual(paired, .paired)
        try recordCredentialDigest("first")
        let reused = try await connection.pairByCode(address: offer.address, code: offer.code, name: phoneName)
        XCTAssertEqual(reused, .freshCode)

        let holder = ConnectionHolder(connection: connection, scope: scope)
        let state = FlowState(holder.state)
        defer { holder.setForeground(active: false) }
        holder.setForeground(active: true)
        try await waitFor(.connected, in: state)
        try signal("revoke")
        _ = try await readFixture("revoked")
        let acknowledged = ContinuousClock.now
        try await waitFor(.notPaired, in: state)
        XCTAssertGreaterThan(ContinuousClock.now - acknowledged, .seconds(7))
        XCTAssertTrue(try store.read() == nil)

        try signal("next")
        let next = try await readFixture("second-qr")
        let nextPairing = try await connection.pairByQr(payload: next, name: phoneName)
        XCTAssertEqual(nextPairing, .paired)
        try recordCredentialDigest("second")
        try await waitFor(.connected, in: state)
    }

    private func makeConnection(
        version: Int32 = PhoneProtocolKt.PHONE_PROTOCOL,
        pollIntervalMillis: Int64 = PhoneConnectionKt.PHONE_POLL_INTERVAL_MILLIS
    ) -> PhoneConnection {
        PhoneConnectionKt.defaultPhoneConnection(
            store: store,
            networkAvailable: { KotlinBoolean(bool: true) },
            protocolVersion: version,
            localAccess: { KotlinBoolean(bool: true) },
            pollIntervalMillis: pollIntervalMillis
        )
    }

    private func requireFixture() throws {
        let marker = directory.appending(path: "enabled")
        try XCTSkipUnless(FileManager.default.fileExists(atPath: marker.path), "Run the pairing fixture script.")
    }

    private func signal(_ event: String) throws {
        try Data().write(to: directory.appending(path: event), options: .atomic)
    }

    private func recordCredentialDigest(_ label: String) throws {
        let credential = try XCTUnwrap(store.read()?.split(separator: "\n").dropFirst(2).first)
        let digest = SHA256.hash(data: Data(credential.utf8)).map { String(format: "%02x", $0) }.joined()
        try digest.write(to: directory.appending(path: "\(label)-digest"), atomically: true, encoding: .utf8)
    }

    private func readOffer(_ file: String) async throws -> (address: String, code: String) {
        let fields = try await readFixture(file).split(separator: "\n")
        guard fields.count == 2 else { throw FixtureFailure.offer }
        return (String(fields[0]), String(fields[1]))
    }

    private func readFixture(_ file: String) async throws -> String {
        let url = directory.appending(path: file)
        for _ in 0..<600 {
            if let input = try? String(contentsOf: url, encoding: .utf8) {
                return input
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw FixtureFailure.offer
    }

    private func waitFor(_ kind: ConnectionStatusKind, in state: FlowState<ConnectionStatus>) async throws {
        for _ in 0..<250 {
            if state.value.kind == kind { return }
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTFail("Expected \(kind.title). Saw \(state.value.kind.title).")
        throw FixtureFailure.state
    }

    private enum FixtureFailure: Error {
        case offer
        case state
    }
}
