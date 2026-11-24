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
        self.network = network
        let checks = Self.computerChecks(network: network)
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
            networkAvailable: { KotlinBoolean(bool: checks.pathSatisfied()) },
            protocolVersion: PhoneProtocolKt.PHONE_PROTOCOL,
            // An iPhone cannot be asked for Local Network access ahead of use, so there is nothing to test
            // before sending. A refusal is found after a failed attempt, by `localNetworkDenied` below.
            localAccess: { KotlinBoolean(bool: true) },
            pollIntervalMillis: checks.pollInterval,
            localNetworkDenied: { host, port in
                KotlinBoolean(bool: LocalNetworkAccess.isDenied(host: host, port: port.int32Value))
            }
        )
        network.start(queue: DispatchQueue(label: "Construct network"))
    }

    /// How the app finds out whether the phone has a network and how often it asks the computer. The
    /// Computer page test (`ComputerPageTests`, run by `scripts/test-phone-pairing.py`) changes both, and
    /// only a debug build in the Simulator can.
    private static func computerChecks(network: NWPathMonitor) -> (pathSatisfied: () -> Bool, pollInterval: Int64) {
        #if DEBUG && targetEnvironment(simulator)
        if ProcessInfo.processInfo.arguments.contains("--computer-page-test") {
            // The test pairs with a program on the loopback address, so the Simulator's network path must not
            // decide the result, and the app asks the computer only every two minutes: only Try again then
            // brings a lost link back.
            // The script also looks for this line in the Simulator log. Finding it shows that the script reads
            // the app's log at all, which makes the check "no secret in the log" more than an empty read.
            Logger().notice("Computer page fixture started")
            return ({ true }, 120_000)
        }
        #endif
        return ({ network.currentPath.status == .satisfied }, PhoneConnectionKt.PHONE_POLL_INTERVAL_MILLIS)
    }

    deinit {
        network.cancel()
    }
}
