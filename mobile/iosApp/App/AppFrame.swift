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
                            if page.showsConnectionLine {
                                ConnectionLine(state: connection, onTryAgain: onTryAgain)
                            }
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
        HStack(spacing: ConstructTheme.Space.small) {
            Circle()
                .fill(ConstructTheme.color(state.kind.dot))
                .frame(width: ConstructTheme.ControlSize.dot, height: ConstructTheme.ControlSize.dot)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: ConstructTheme.Space.small) {
                Text(state.kind.title)
                if let detail = state.kind.detail {
                    Text(detail)
                    if let time = LastContact.date(millis: state.lastContactMillis) {
                        Text(LastContact.text(time))
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

extension ConnectionStatusKind {
    /// The dot of a state, as on the Android page: green when connected, red when the computer
    /// cannot be reached, grey while the state is not known.
    var dot: ColorRole {
        switch self {
        case .connected: return .success
        case .checking, .notPaired: return .borderStrong
        default: return .destructive
        }
    }
}
