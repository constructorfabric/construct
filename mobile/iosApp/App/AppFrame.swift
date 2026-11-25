import SwiftUI

/// The frame every page sits in: the tab bar, and for each page a navigation bar with the page's
/// title. `content` draws one page. A page that has something to sync gets the sync's notice in its environment, and
/// shows it as the first thing of its content; nothing shows while all is fine.
struct AppFrame<Content: View>: View {
    let syncNotice: SyncNoticeContent
    @Binding var selected: Page
    @ViewBuilder let content: (Page) -> Content

    var body: some View {
        TabView(selection: $selected) {
            ForEach(Page.allCases) { page in
                NavigationStack {
                    content(page)
                        .environment(\.syncNotice, page.showsSyncNotice ? syncNotice : nil)
                        .navigationTitle(page.title)
                        .navigationBarTitleDisplayMode(.inline)
                }
                .tabItem { Label(page.title, image: page.icon) }
                .tag(page)
            }
        }
        .tint(ConstructTheme.color(.linkForeground))
    }
}
