import Combine
import SwiftUI

/// The shell: the frame and the page each tab shows. A page with a feature is a folder under
/// `Features/`; one without is a `PlaceholderPage` until its feature takes it over.
struct RootView: View {
    let composition: Composition
    @State private var selected = Page.chat

    init(composition: Composition) {
        self.composition = composition
        ConstructTheme.styleBars()
    }

    var body: some View {
        AppFrame(connection: ConnectionState.current(), selected: $selected) { page in
            switch page {
            case .chat:
                ChatPage(
                    construct: composition.construct,
                    settings: composition.modelSettings,
                    settingsChanges: composition.modelSettingsDidCommit.eraseToAnyPublisher(),
                    active: selected == .chat,
                    openSettings: { selected = .settings }
                )
            case .memory: MemoryPage(construct: composition.construct)
            case .settings:
                SettingsPage(settings: composition.modelSettings) {
                    composition.modelSettingsDidCommit.send(())
                }
            default: PlaceholderPage(page: page)
            }
        }
    }
}
