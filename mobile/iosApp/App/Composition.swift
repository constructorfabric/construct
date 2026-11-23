import ConstructShared
import SwiftUI

/// The one place the app's objects are built. To run the app on `FakeConstruct` instead of the
/// phone's copy, change `makeConstruct()` and nothing else.
final class Composition: ObservableObject {
    let construct: Construct
    /// The model setting: the address and the model name in the app's preferences, the key in the Keychain.
    let modelSettings: ModelSettingsStore

    init() {
        construct = Composition.makeConstruct()
        modelSettings = IosModelSettingsKt.iosModelSettingsStore(
            defaults: UserDefaults.standard,
            service: "construct.model"
        )
    }

    private static func makeConstruct() -> Construct {
        let store = CopyStore(driver: NativeDriverKt.nativeCopyDriver(name: CopyFileKt.COPY_FILE))
        #if DEBUG
        // There is no sync yet, so a debug build fills an empty copy with the sample data.
        _ = store.seedWithSampleData()
        #endif
        return DatabaseConstruct(store: store, dispatcher: BackgroundKt.backgroundDispatcher())
    }
}
