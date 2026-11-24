import SwiftUI

/// The frame every page sits in: the tab bar, and for each page a navigation bar with the page's
/// title and the connection line under it. `content` draws one page.
struct AppFrame<Content: View>: View {
    let connection: ConnectionState
    @Binding var selected: Page
    @ViewBuilder let content: (Page) -> Content

    var body: some View {
        TabView(selection: $selected) {
            ForEach(Page.allCases) { page in
                NavigationStack {
                    content(page)
                        .safeAreaInset(edge: .top, spacing: 0) { ConnectionLine(state: connection) }
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

/// One line under the navigation bar: a dot and the connection state in words.
struct ConnectionLine: View {
    let state: ConnectionState

    var body: some View {
        let connected = state == .connected
        HStack(spacing: ConstructTheme.Space.small) {
            Circle()
                .fill(ConstructTheme.color(connected ? .primary : .borderStrong))
                .frame(width: ConstructTheme.ControlSize.dot, height: ConstructTheme.ControlSize.dot)
            Text(state.text)
                .font(.system(size: ConstructTheme.TextSize.caption))
                .foregroundColor(ConstructTheme.color(.mutedForeground))
            Spacer(minLength: 0)
        }
        .padding(.horizontal, ConstructTheme.Space.large)
        .padding(.vertical, ConstructTheme.Space.small)
        .frame(maxWidth: .infinity)
        .background(ConstructTheme.color(.surface))
        .accessibilityElement(children: .combine)
    }
}
