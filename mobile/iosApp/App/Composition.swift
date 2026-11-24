import ConstructShared
import SwiftUI

/// The one place the app's objects are built. To run the app on `FakeConstruct` instead of the
/// phone's copy, change `makeConstruct()` and nothing else.
final class Composition: ObservableObject {
    let construct: Construct
    /// The provider setting stored together in the Keychain.
    let modelSettings: ModelSettingsStore

    init() {
        let settings = IosModelSettingsKt.iosModelSettingsStore(
            defaults: UserDefaults.standard,
            service: "construct.model"
        )
        modelSettings = settings
        construct = Composition.makeConstruct(settings: settings)
    }

    private static func makeConstruct(settings: ModelSettingsStore) -> Construct {
        let store = CopyStore(driver: NativeDriverKt.nativeCopyDriver(name: CopyFileKt.COPY_FILE))
        #if DEBUG
        // There is no sync yet, so a debug build fills an empty copy with the sample data.
        _ = store.seedWithSampleData()
        #endif
        return PhoneConstruct(store: store, settings: settings, dispatcher: BackgroundKt.backgroundDispatcher())
    }
}
