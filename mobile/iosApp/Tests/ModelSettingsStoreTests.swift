import ConstructShared
import Security
import XCTest

/// The model setting on the iPhone, over the real Keychain and real preferences. The Keychain is only
/// there inside the app, so these run in the app's test host, not in the Kotlin test process.
final class ModelSettingsStoreTests: XCTestCase {
    private let fakeKey = "sk-fake-0123456789-do-not-use"
    private let address = "http://127.0.0.1:9/v1"
    private let service = "construct.model.test.\(UUID().uuidString)"
    private let suite = "construct.model.test.\(UUID().uuidString)"

    private func openStore() -> ModelSettingsStore {
        IosModelSettingsKt.iosModelSettingsStore(defaults: UserDefaults(suiteName: suite)!, service: service)
    }

    override func tearDown() {
        SecItemDelete([kSecClass: kSecClassGenericPassword, kSecAttrService: service] as CFDictionary)
        UserDefaults().removePersistentDomain(forName: suite)
    }

    func testAnEmptyPhoneHasNothingSet() throws {
        XCTAssertEqual(try openStore().read(), ModelSettings(address: "", model: "", keyIsSet: false))
        XCTAssertNil(try openStore().readProvider().key)
    }

    func testTheTextAndTheKeySurviveAReopen() throws {
        _ = try openStore().saveProvider(address: address, model: "demo-model", key: fakeKey)

        let again = openStore()
        XCTAssertEqual(
            try again.read(),
            ModelSettings(address: address, model: "demo-model", keyIsSet: true)
        )
        XCTAssertEqual(try again.readProvider().key, fakeKey)
    }

    func testANewKeyReplacesTheOldOneAndARemovedKeyIsGone() throws {
        let store = openStore()
        _ = try store.saveProvider(address: address, model: "demo-model", key: fakeKey)
        _ = try store.saveProvider(address: address, model: "demo-model", key: "sk-other-9876543210-do-not-use")
        XCTAssertEqual(try openStore().readProvider().key, "sk-other-9876543210-do-not-use")

        try store.removeKey()

        XCTAssertNil(try openStore().readProvider().key)
        XCTAssertFalse(try openStore().read().keyIsSet)
        try store.removeKey()
    }

    func testTheKeyItemStaysOnThisDeviceAndOutOfBackups() throws {
        _ = try openStore().saveProvider(address: address, model: "demo-model", key: fakeKey)
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
        _ = try store.saveProvider(address: address, model: "first-model", key: odd)
        _ = try store.saveProvider(address: address, model: "demo-model", key: nil)
        XCTAssertEqual(try openStore().readProvider().key, odd)
        XCTAssertFalse(String(describing: try openStore().read()).contains(odd))
        try store.removeKey()
        XCTAssertEqual(try openStore().read().model, "demo-model")
        XCTAssertNil(try openStore().readProvider().key)
    }

    func testLegacyPreferencesDoNotAttachAnUnscopedKey() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defaults.set(address, forKey: "model_address")
        defaults.set("old-model", forKey: "model_name")
        let legacy: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: "api-key",
            kSecValueData: Data("construct-model-v1:legacy-key".utf8)
        ]
        XCTAssertEqual(SecItemAdd(legacy as CFDictionary, nil), errSecSuccess)

        let store = openStore()
        XCTAssertEqual(try store.read(), ModelSettings(address: address, model: "old-model", keyIsSet: false))
        XCTAssertNil(try store.readProvider().key)
        var legacyLookup = legacy
        legacyLookup.removeValue(forKey: kSecValueData)
        XCTAssertEqual(SecItemCopyMatching(legacyLookup as CFDictionary, nil), errSecItemNotFound)

        _ = try store.saveProvider(address: address, model: "new-model", key: fakeKey)
        XCTAssertEqual(try openStore().readProvider().key, fakeKey)
        try store.removeKey()
        XCTAssertEqual(try openStore().read(), ModelSettings(address: address, model: "new-model", keyIsSet: false))
        XCTAssertEqual(SecItemCopyMatching(legacyLookup as CFDictionary, nil), errSecItemNotFound)
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
            _ = try store.saveProvider(address: address, model: "demo-model", key: fakeKey)
        case "remove":
            XCTAssertEqual(try store.read().model, "demo-model")
            XCTAssertEqual(try store.readProvider().key, fakeKey)
            try store.removeKey()
        case "verify":
            XCTAssertNil(try store.readProvider().key)
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
        _ = try store.saveProvider(address: address, model: "demo-model", key: key)
        XCTAssertTrue(try store.read().keyIsSet)
    }
}
