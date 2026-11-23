import ConstructShared
import SwiftUI

/// The frame every page sits in: the tab bar, and for each page a navigation bar with the page's
/// title and the connection line under it. `content` draws one page.
struct AppFrame<Content: View>: View {
    let connection: ConnectionStatus
    let onTryAgain: () -> Void
    @Binding var selected: Page
    @ViewBuilder let content: (Page) -> Content

    var body: some View {
        TabView(selection: $selected) {
            ForEach(Page.allCases) { page in
                NavigationStack {
                    content(page)
                        .safeAreaInset(edge: .top, spacing: 0) {
                            ConnectionLine(state: connection, onTryAgain: onTryAgain)
                        }
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

struct ConnectionLine: View {
    let state: ConnectionStatus
    let onTryAgain: () -> Void

    var body: some View {
        let connected = state.kind == .connected
        HStack(spacing: ConstructTheme.Space.small) {
            Circle()
                .fill(ConstructTheme.color(connected ? .primary : .borderStrong))
                .frame(width: ConstructTheme.ControlSize.dot, height: ConstructTheme.ControlSize.dot)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: ConstructTheme.Space.small) {
                Text(state.kind.title)
                if let detail = state.kind.detail {
                    Text(detail)
                    if state.lastContactMillis > 0 {
                        let time = Date(timeIntervalSince1970: Double(state.lastContactMillis) / 1000)
                        Text("Last contact: \(time.formatted(date: .abbreviated, time: .shortened))")
                    }
                }
            }
            .font(.system(size: ConstructTheme.TextSize.caption))
            .foregroundColor(ConstructTheme.color(.mutedForeground))
            Spacer(minLength: 0)
            if state.kind.canRetry {
                Button("Try again", action: onTryAgain)
            }
        }
        .padding(.horizontal, ConstructTheme.Space.large)
        .padding(.vertical, ConstructTheme.Space.small)
        .frame(maxWidth: .infinity)
        .background(ConstructTheme.color(.surface))
    }
}
