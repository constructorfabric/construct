import Combine
import ConstructShared
import Network
import SwiftUI

/// The one place the app's objects are built. To run the app on `FakeConstruct` instead of the
/// phone's copy, change `makeConstruct()` and nothing else.
final class Composition: ObservableObject {
    let construct: Construct
    /// The provider setting stored together in the Keychain.
    let modelSettings: ModelSettingsStore
    let modelSettingsDidCommit = PassthroughSubject<Void, Never>()
    let phoneConnection: PhoneConnection
    private let network: NWPathMonitor

    init() {
        let network = NWPathMonitor()
        let frameFixture = Self.isPairingFrameFixture
        self.network = network
        var service = "construct.model"
        var defaults = UserDefaults.standard
        var copyName = CopyFileKt.COPY_FILE
        var seed = false
        #if DEBUG
        let environment = ProcessInfo.processInfo.environment
        seed = true
        // UI tests exercise the real Keychain and database in their own namespace.
        if let id = environment["CONSTRUCT_UI_TEST_ID"].flatMap(UUID.init(uuidString:)) {
            service += ".test.\(id.uuidString)"
            defaults = UserDefaults(suiteName: service) ?? .standard
            copyName = "ui-test-\(id.uuidString).sqlite"
            seed = environment["CONSTRUCT_UI_TEST_EMPTY_COPY"] != "1"
        }
        #endif
        let settings = IosModelSettingsKt.iosModelSettingsStore(
            defaults: defaults,
            service: service
        )
        modelSettings = settings
        construct = Composition.makeConstruct(settings: settings, copyName: copyName, seed: seed)
        phoneConnection = PhoneConnectionKt.defaultPhoneConnection(
            store: IosPairingSecretKt.iosPairingSecret(service: "construct.computer"),
            networkAvailable: { KotlinBoolean(bool: frameFixture || network.currentPath.status == .satisfied) },
            protocolVersion: PhoneProtocolKt.PHONE_PROTOCOL,
            localAccess: { KotlinBoolean(bool: true) },
            pollIntervalMillis: frameFixture ? 120_000 : PhoneConnectionKt.PHONE_POLL_INTERVAL_MILLIS
        )
        network.start(queue: DispatchQueue(label: "Construct network"))
    }

    private static var isPairingFrameFixture: Bool {
        #if DEBUG
        ProcessInfo.processInfo.arguments.contains("--pairing-frame-test")
        #else
        false
        #endif
    }

    deinit {
        network.cancel()
    }

    private static func makeConstruct(settings: ModelSettingsStore, copyName: String, seed: Bool) -> Construct {
        let store = CopyStore(driver: NativeDriverKt.nativeCopyDriver(name: copyName))
        // There is no sync yet, so a debug build fills an empty copy with the sample data.
        if seed { _ = store.seedWithSampleData() }
        return PhoneConstruct(store: store, settings: settings, dispatcher: BackgroundKt.backgroundDispatcher())
    }
}
