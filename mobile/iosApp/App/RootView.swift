import SwiftUI

/// The shell: the theme's background and the page the app opens on. A page is a folder under
/// `Features/`; when the app has more than one, the navigation goes here.
struct RootView: View {
    let composition: Composition

    var body: some View {
        NavigationStack {
            MemoryPage(construct: composition.construct)
                .navigationTitle("Memory")
                .navigationBarTitleDisplayMode(.inline)
        }
        .tint(ConstructTheme.color(.primary))
    }
}
