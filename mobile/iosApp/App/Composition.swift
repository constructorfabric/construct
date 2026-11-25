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
    /// Copies from the paired computer into `copy`, and how often and how long the app lets it show.
    let syncer: Syncer
    let syncTiming: SyncTiming
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
        // Pairing records the computer in the copy (the same computer keeps what the phone holds of it, another one
        // empties it); that call stays inside Kotlin, see `iosPhoneConnection`.
        phoneConnection = IosPhoneConnectionKt.iosPhoneConnection(
            copy: store,
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
        let sync = Self.sync(IosSyncKt.iosSyncer(connection: phoneConnection, copy: store))
        syncer = sync.syncer
        syncTiming = sync.timing
        network.start(queue: DispatchQueue(label: "Construct network"))
    }

    /// The sync with its timing: the app's own, or, for the sync test of the Simulator, a sync that waits before each
    /// step so a test can see the loading screen and its counts, and checks often so it need not wait a minute. A test
    /// that must not meet a check in the middle of what it looks at sets the time between the checks (in
    /// milliseconds) in `CONSTRUCT_SYNC_TEST_INTERVAL_MS`.
    private static func sync(_ syncer: Syncer) -> (syncer: Syncer, timing: SyncTiming) {
        #if DEBUG && targetEnvironment(simulator)
        if ProcessInfo.processInfo.arguments.contains("--sync-test") {
            Logger().notice("Sync fixture started")
            let interval = ProcessInfo.processInfo.environment["CONSTRUCT_SYNC_TEST_INTERVAL_MS"].flatMap(Int64.init)
            return (PacedSyncer(syncer, pause: 0.9), SyncTiming(interval: interval ?? 6000, justSynced: 3000))
        }
        #endif
        let timing = SyncTiming(
            interval: SyncHolderKt.SYNC_INTERVAL_MILLIS,
            justSynced: SyncHolderKt.JUST_SYNCED_MILLIS
        )
        return (syncer, timing)
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

/// How often the app checks the computer while it is in front of the person, and how long "Synced just now" shows,
/// in milliseconds.
struct SyncTiming {
    let interval: Int64
    let justSynced: Int64
}

#if DEBUG && targetEnvironment(simulator)
/// For the sync test only: waits before each sync (three times `pause`) and after each report of progress, so the test
/// can see "Syncing" and the counts of the first copy rise.
final class PacedSyncer: NSObject, Syncer {
    private let base: Syncer
    private let pause: TimeInterval

    init(_ base: Syncer, pause: TimeInterval) {
        self.base = base
        self.pause = pause
    }

    // The requirement as SKIE names it for a Kotlin `suspend` function of an interface.
    // swiftlint:disable:next identifier_name
    func __sync(progress: @escaping (StepProgress) -> Void) async throws -> SyncResult {
        // Long enough for a test that looks every second or so to see "Syncing" at all.
        try await Task.sleep(nanoseconds: UInt64(pause * 3 * 1_000_000_000))
        return try await base.sync { [pause] report in
            progress(report)
            Thread.sleep(forTimeInterval: pause)
        }
    }

    func record() -> SyncRecord {
        base.record()
    }
}
#endif
