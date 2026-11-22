import SwiftUI

/// The app's entry point. The window shows the shell, which owns the objects of the features.
@main
struct ConstructApp: App {
    @StateObject private var composition = Composition()

    var body: some Scene {
        WindowGroup {
            RootView(composition: composition)
        }
    }
}
