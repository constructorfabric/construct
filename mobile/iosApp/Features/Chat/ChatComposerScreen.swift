import ConstructShared
import SwiftUI

struct ChatComposerScreen: View {
    let state: ChatState
    let actions: ChatActions
    let available: Bool
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: ConstructTheme.Space.small) {
            HStack(spacing: ConstructTheme.Space.none) {
                TextField("Message Construct", text: Binding(get: actions.draft, set: actions.edit), axis: .vertical)
                    .focused($focused)
                    .lineLimit(1...5)
                    .foregroundStyle(ConstructTheme.color(.foreground))
                    .disabled(!available || !state.modelReady || state.deleting)
                    .padding(.vertical, ConstructTheme.Space.small)
                if state.answering {
                    ChatComposerButton(
                        title: "Stop",
                        icon: "ChatStop",
                        iconSize: ConstructTheme.ControlSize.smallIcon,
                        enabled: !state.stopping,
                        filled: false,
                        action: actions.stop
                    )
                }
                ChatComposerButton(
                    title: "Send",
                    icon: "ChatSend",
                    iconSize: ConstructTheme.ControlSize.sendIcon,
                    enabled: canSend,
                    filled: true,
                    action: send
                )
            }
            .padding(.leading, ConstructTheme.Space.large)
            .padding(.trailing, ConstructTheme.Space.extraSmall)
            .frame(minHeight: ConstructTheme.ControlSize.composer)
            .overlay {
                RoundedRectangle(cornerRadius: ConstructTheme.Radius.large)
                    .strokeBorder(ConstructTheme.color(.borderStrong), lineWidth: ConstructTheme.ControlSize.stroke)
                    .allowsHitTesting(false)
            }
            Text(state.modelName.isEmpty ? "Model: not set" : "Model: \(state.modelName)")
                .font(.caption)
                .foregroundStyle(ConstructTheme.color(.mutedForeground))
                .frame(maxWidth: .infinity)
                .multilineTextAlignment(.center)
        }
        .padding(.bottom, ConstructTheme.Space.extraSmall)
    }

    private var canSend: Bool {
        available && state.modelReady && !state.answering && !state.deleting
            && !state.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func send() {
        actions.send(actions.draft())
        focused = false
    }
}

private struct ChatComposerButton: View {
    let title: String
    let icon: String
    let iconSize: CGFloat
    let enabled: Bool
    let filled: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            FeatureIcon(icon, size: iconSize)
                .foregroundStyle(ConstructTheme.color(enabled && filled ? .primaryForeground : .mutedForeground))
                .frame(
                    width: ConstructTheme.ControlSize.composerButton,
                    height: ConstructTheme.ControlSize.composerButton
                )
                .background(ConstructTheme.color(enabled && filled ? .primary : .muted), in: Circle())
                .overlay {
                    if !enabled || !filled {
                        Circle().strokeBorder(
                            ConstructTheme.color(.borderStrong),
                            lineWidth: ConstructTheme.ControlSize.stroke
                        )
                    }
                }
                .frame(width: ConstructTheme.ControlSize.touch, height: ConstructTheme.ControlSize.touch)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .disabled(!enabled)
    }
}
