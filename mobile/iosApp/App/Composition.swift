import ConstructShared
import SwiftUI

/// The one place the app's objects are built. To run the app on `FakeConstruct` instead of the
/// phone's copy, change `makeConstruct()` and nothing else.
final class Composition: ObservableObject {
    let construct: Construct

    init() {
        construct = Composition.makeConstruct()
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
