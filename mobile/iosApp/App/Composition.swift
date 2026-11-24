import Combine
import ConstructShared
import Network
import OSLog
import SwiftUI

/// The one place the app's objects are built. To run the app on `FakeConstruct` instead of the
/// phone's copy, substitute it at the `construct` assignment below.
final class Composition: ObservableObject {
    let construct: Construct
    /// The provider setting stored together in the Keychain.
    let modelSettings: ModelSettingsStore
    let modelSettingsDidCommit = PassthroughSubject<Void, Never>()
    let phoneConnection: PhoneConnection
    let copy: CopyStore
    private let network: NWPathMonitor

    init() {
        let network = NWPathMonitor()
        let frameFixture = Self.isPairingFrameFixture
        let computerFixture = Self.isComputerPageFixture
        let pairingFixture = frameFixture || computerFixture
        self.network = network
        var service = "construct.model"
        var pairingService = "construct.computer"
        var defaults = UserDefaults.standard
        var copyName = CopyFileKt.COPY_FILE
        var seed = false
        #if DEBUG
        let environment = ProcessInfo.processInfo.environment
        seed = true
        // UI tests exercise the real Keychain and database in their own namespace.
        if let id = environment["CONSTRUCT_UI_TEST_ID"].flatMap(UUID.init(uuidString:)) {
            service += ".test.\(id.uuidString)"
            pairingService += ".test.\(id.uuidString)"
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
        let store = CopyStore(driver: NativeDriverKt.nativeCopyDriver(name: copyName))
        // Seed only once so an unpair stays cleared when a debug build is reopened.
        if seed && !defaults.bool(forKey: "sampleCopySeeded") {
            _ = store.seedWithSampleData()
            defaults.set(true, forKey: "sampleCopySeeded")
        }
        copy = store
        construct = PhoneConstruct(store: store, settings: settings, dispatcher: BackgroundKt.backgroundDispatcher())
        phoneConnection = PhoneConnectionKt.defaultPhoneConnection(
            store: IosPairingSecretKt.iosPairingSecret(service: pairingService),
            networkAvailable: {
                KotlinBoolean(bool: pairingFixture || network.currentPath.status == .satisfied)
            },
            protocolVersion: PhoneProtocolKt.PHONE_PROTOCOL,
            localAccess: { KotlinBoolean(bool: true) },
            pollIntervalMillis: pairingFixture ? 120_000 : PhoneConnectionKt.PHONE_POLL_INTERVAL_MILLIS,
            localNetworkDenied: { host, port in
                KotlinBoolean(bool: LocalNetworkAccess.isDenied(host: host, port: port.int32Value))
            }
        )
        network.start(queue: DispatchQueue(label: "Construct network"))
        if computerFixture { Logger().notice("Computer page fixture started") }
    }

    private static var isPairingFrameFixture: Bool {
        #if DEBUG
        ProcessInfo.processInfo.arguments.contains("--pairing-frame-test")
        #else
        false
        #endif
    }

    private static var isComputerPageFixture: Bool {
        #if DEBUG && targetEnvironment(simulator)
        ProcessInfo.processInfo.arguments.contains("--computer-page-test")
        #else
        false
        #endif
    }

    deinit {
        network.cancel()
    }
}
