import ConstructShared
import Security
import XCTest

/// The model setting on the iPhone, over the real Keychain and real preferences. The Keychain is only
/// there inside the app, so these run in the app's test host, not in the Kotlin test process.
final class ModelSettingsStoreTests: XCTestCase {
    private let fakeKey = "sk-fake-0123456789-do-not-use"
    private let service = "construct.model.test.\(UUID().uuidString)"
    private let suite = "construct.model.test.\(UUID().uuidString)"

    private func openStore() -> ModelSettingsStore {
        IosModelSettingsKt.iosModelSettingsStore(defaults: UserDefaults(suiteName: suite)!, service: service)
    }

    override func tearDown() {
        try? openStore().removeKey()
        UserDefaults().removePersistentDomain(forName: suite)
    }

    func testAnEmptyPhoneHasNothingSet() throws {
        XCTAssertEqual(try openStore().read(), ModelSettings(address: "", model: "", keyIsSet: false))
        XCTAssertNil(try openStore().readKey())
    }

    func testTheTextAndTheKeySurviveAReopen() throws {
        try openStore().saveText(address: "http://127.0.0.1:8080/v1", model: "demo-model")
        try openStore().saveKey(key: fakeKey)

        let again = openStore()
        XCTAssertEqual(
            try again.read(),
            ModelSettings(address: "http://127.0.0.1:8080/v1", model: "demo-model", keyIsSet: true)
        )
        XCTAssertEqual(try again.readKey(), fakeKey)
    }

    func testANewKeyReplacesTheOldOneAndARemovedKeyIsGone() throws {
        let store = openStore()
        try store.saveKey(key: fakeKey)
        try store.saveKey(key: "sk-other-9876543210-do-not-use")
        XCTAssertEqual(try openStore().readKey(), "sk-other-9876543210-do-not-use")

        try store.removeKey()

        XCTAssertNil(try openStore().readKey())
        XCTAssertFalse(try openStore().read().keyIsSet)
        try store.removeKey()
    }

    func testTheKeyItemStaysOnThisDeviceAndOutOfBackups() throws {
        try openStore().saveKey(key: fakeKey)
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrSynchronizable: kSecAttrSynchronizableAny,
            kSecReturnAttributes: true
        ]
        var found: CFTypeRef?

        XCTAssertEqual(SecItemCopyMatching(query as CFDictionary, &found), errSecSuccess)

        let attributes = try XCTUnwrap(found as? [CFString: Any])
        XCTAssertEqual(
            attributes[kSecAttrAccessible] as? String,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly as String
        )
        // 0 or absent: the item does not go to iCloud Keychain.
        XCTAssertTrue((attributes[kSecAttrSynchronizable] as? Bool ?? false) == false)
    }

    func testTextChangesKeepAUnicodeKeyAndRemovalKeepsText() throws {
        let odd = "sk-ключ \"quoted\" 'x' \n tail ✓"
        let store = openStore()
        try store.saveKey(key: odd)
        try store.saveText(address: "http://127.0.0.1:9/v1", model: "demo-model")
        XCTAssertEqual(try openStore().readKey(), odd)
        XCTAssertFalse(String(describing: try openStore().read()).contains(odd))
        try store.removeKey()
        XCTAssertEqual(try openStore().read().model, "demo-model")
        XCTAssertNil(try openStore().readKey())
    }

    /// Run in separate test-host launches for each phase to verify persistence across a process restart.
    func testSettingsAcrossProcessRestarts() throws {
        guard let phase = ProcessInfo.processInfo.environment["RESTART_CHECK"] else {
            throw XCTSkip("only the restart check sets this")
        }
        let name = "construct.model.restart-test"
        let defaults = UserDefaults(suiteName: name)!
        let store = IosModelSettingsKt.iosModelSettingsStore(defaults: defaults, service: name)
        switch phase {
        case "save":
            try store.saveText(address: "http://127.0.0.1:9/v1", model: "demo-model")
            try store.saveKey(key: fakeKey)
        case "remove":
            XCTAssertEqual(try store.read().model, "demo-model")
            XCTAssertEqual(try store.readKey(), fakeKey)
            try store.removeKey()
        case "verify":
            XCTAssertNil(try store.readKey())
            XCTAssertFalse(try store.read().keyIsSet)
            defaults.removePersistentDomain(forName: name)
        default:
            XCTFail("Unknown restart check phase")
        }
    }

    /// Run by the leak search: saves its shared fake key and leaves it saved.
    func testLeavesAFakeKeyForTheLeakSearch() throws {
        guard ProcessInfo.processInfo.environment["LEAK_SEARCH"] == "1" else {
            throw XCTSkip("only the leak search script sets this")
        }
        let key = "sk-ios-leak-search-0123456789-do-not-use"
        let store = IosModelSettingsKt.iosModelSettingsStore(
            defaults: UserDefaults.standard,
            service: "construct.model"
        )
        try store.saveText(address: "http://127.0.0.1:9/v1", model: "demo-model")
        try store.saveKey(key: key)
        XCTAssertTrue(try store.read().keyIsSet)
    }
}
