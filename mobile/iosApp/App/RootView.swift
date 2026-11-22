import SwiftUI

/// The shell: the frame and the page each tab shows. A page with a feature is a folder under
/// `Features/`; one without is a `PlaceholderPage` until its feature takes it over.
struct RootView: View {
    let composition: Composition

    init(composition: Composition) {
        self.composition = composition
        ConstructTheme.styleBars()
    }

    var body: some View {
        AppFrame(connection: ConnectionState.current()) { page in
            switch page {
            case .memory: MemoryPage(construct: composition.construct)
            default: PlaceholderPage(page: page)
            }
        }
    }
}
