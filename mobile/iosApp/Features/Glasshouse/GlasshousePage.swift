import SwiftUI

/// Shows `GlasshouseScreen` with the state and the web view of the model the shell keeps. The page has no title bar
/// and no connection line: its address bar takes their place.
struct GlasshousePage: View {
    @ObservedObject var model: GlasshouseModel

    var body: some View {
        GlasshouseScreen(
            state: model.state,
            progress: model.progress,
            webView: model.webView,
            actions: model.actions
        )
        .toolbar(.hidden, for: .navigationBar)
        .onAppear(perform: model.startIfNeeded)
    }
}
