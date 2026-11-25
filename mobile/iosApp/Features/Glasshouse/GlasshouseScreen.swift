import ConstructShared
import SwiftUI
import WebKit

struct GlasshouseActions {
    /// Opens what the person typed and says whether it opened; an address that is not a web one is refused.
    let openAddress: (String) -> Bool
    let edited: () -> Void
    let reload: () -> Void
    let back: () -> Void
    let forward: () -> Void
}

/// The Glasshouse page: the address bar under the status area, the web page, and back and forward above the tab
/// bar. It draws the state and the web view it is given, and loads nothing itself.
struct GlasshouseScreen: View {
    @ObservedObject var state: FlowState<GlasshouseState>
    let progress: Double
    let webView: WKWebView?
    let actions: GlasshouseActions

    var body: some View {
        let current = state.value
        VStack(spacing: ConstructTheme.Space.none) {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.small) {
                GlasshouseAddressBar(
                    address: current.address,
                    secure: current.problem == nil && current.address.lowercased().hasPrefix("https://"),
                    loading: current.loading,
                    progress: progress,
                    openAddress: actions.openAddress,
                    edited: actions.edited,
                    reload: actions.reload
                )
                if current.addressRefused {
                    InlineNotice(text: GlasshouseTexts.notWeb, error: true)
                        .accessibilityIdentifier("glasshouse-message")
                }
            }
            .padding(.horizontal, ConstructTheme.Space.large)
            .padding(.vertical, ConstructTheme.Space.small)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ConstructTheme.color(.surface).ignoresSafeArea(edges: .top))
            page(current)
            GlasshouseToolbar(
                canGoBack: current.canGoBack,
                canGoForward: current.canGoForward,
                back: actions.back,
                forward: actions.forward
            )
        }
        .background(ConstructTheme.color(.background))
    }

    /// The web page, with the message that says why there is none in front of it when there is a problem. The web
    /// view stays in place under the message, so it is the same one when the page opens again.
    private func page(_ current: GlasshouseState) -> some View {
        let message = messageText(current)
        return ZStack(alignment: .topLeading) {
            if let webView {
                GlasshouseWebContainer(webView: webView)
                    .accessibilityHidden(message != nil)
            }
            if let message {
                VStack(alignment: .leading, spacing: ConstructTheme.Space.none) {
                    InlineNotice(
                        text: message.text,
                        error: message.error,
                        actions: message.error ? [.init(title: GlasshouseTexts.reload, run: actions.reload)] : []
                    )
                    .accessibilityIdentifier("glasshouse-message")
                    .padding(ConstructTheme.Space.large)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .background(ConstructTheme.color(.background))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func messageText(_ current: GlasshouseState) -> (text: String, error: Bool)? {
        if let problem = current.problem {
            return (GlasshouseTexts.problem(problem), true)
        }
        if current.noStartAddress && current.address.isEmpty {
            return (GlasshouseTexts.noStartAddress, false)
        }
        return nil
    }
}

/// Back and forward, above the tab bar. An arrow with no page to go to is dimmed and does nothing.
private struct GlasshouseToolbar: View {
    let canGoBack: Bool
    let canGoForward: Bool
    let back: () -> Void
    let forward: () -> Void

    var body: some View {
        HStack(spacing: ConstructTheme.Space.none) {
            arrow("GlasshouseBack", label: "Back", enabled: canGoBack, action: back)
            Spacer(minLength: ConstructTheme.Space.none)
            arrow("GlasshouseForward", label: "Forward", enabled: canGoForward, action: forward)
        }
        .padding(.horizontal, ConstructTheme.Space.small)
        .frame(height: ConstructTheme.ControlSize.touch)
        .frame(maxWidth: .infinity)
        .background(ConstructTheme.color(.background))
        .overlay(alignment: .top) {
            Rectangle()
                .fill(ConstructTheme.color(.border))
                .frame(height: ConstructTheme.ControlSize.stroke)
        }
    }

    private func arrow(_ icon: String, label: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            FeatureIcon(icon)
                .frame(width: ConstructTheme.ControlSize.touch, height: ConstructTheme.ControlSize.touch)
        }
        .disabled(!enabled)
        .foregroundStyle(enabled ? ConstructTheme.color(.linkForeground) : ConstructTheme.color(.mutedForeground))
        .opacity(enabled ? 1 : ConstructTheme.Loading.dimmed)
        .accessibilityLabel(label)
        .accessibilityIdentifier("glasshouse-\(label.lowercased())")
    }
}
