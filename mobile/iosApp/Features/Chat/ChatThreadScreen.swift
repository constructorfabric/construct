import ConstructShared
import SwiftUI

struct ChatThreadScreen: View {
    let state: ChatState
    let thread: ChatThread
    let actions: ChatActions
    let openSettings: () -> Void
    @State private var confirmDelete = false

    var body: some View {
        Group {
            if thread.loading {
                ProgressView("Loading the conversation")
            } else if thread.loadFailed {
                InlineNotice(text: "The conversation cannot be shown: the connection was lost.", error: true)
                    .padding(ConstructTheme.Space.large)
                    .frame(maxHeight: .infinity, alignment: .top)
            } else if thread.messages.isEmpty && thread.reply == nil && !thread.readOnly {
                newChat
            } else {
                ChatMessagesScreen(state: state, thread: thread, actions: actions, openSettings: openSettings)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .safeAreaInset(edge: .bottom, spacing: ConstructTheme.Space.none) { footer }
        .background(ConstructTheme.color(.background))
        .toolbar {
            if !thread.readOnly && !thread.id.isEmpty && !thread.loading && !thread.loadFailed {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Menu {
                        Button("Delete", role: .destructive) { confirmDelete = true }
                    } label: {
                        FeatureIcon("ChatMore")
                    }
                    .accessibilityLabel("Conversation actions")
                    .disabled(state.answering || state.deleting)
                }
            }
        }
        .alert("Delete this conversation?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { actions.delete(thread.id) }
        }
    }

    private var newChat: some View {
        GeometryReader { space in
            ScrollView {
                VStack(spacing: ConstructTheme.Space.small) {
                    Text("What can I help with?")
                        .font(.system(size: ConstructTheme.TextSize.heading, weight: .semibold))
                        .multilineTextAlignment(.center)
                        .padding(.bottom, ConstructTheme.Space.small)
                    example("What do you know about me?")
                    example("I mostly write Rust")
                }
                .frame(maxWidth: .infinity, minHeight: space.size.height)
                .padding(.horizontal, ConstructTheme.Space.large)
            }
        }
    }

    private func example(_ prompt: String) -> some View {
        Button {
            actions.send(prompt)
        } label: {
            Text(prompt)
                .font(.system(size: ConstructTheme.TextSize.secondary, weight: .semibold))
                .padding(.horizontal, ConstructTheme.Space.large)
                .frame(minHeight: ConstructTheme.ControlSize.touch)
                .background {
                    Capsule().strokeBorder(ConstructTheme.color(.border), lineWidth: ConstructTheme.ControlSize.stroke)
                }
        }
        .disabled(!state.modelReady || state.answering || state.deleting)
    }

    @ViewBuilder
    private var footer: some View {
        if thread.readOnly {
            InlineNotice(text: "This chat was started on a computer and is read-only.", icon: "ChatComputer")
                .padding(ConstructTheme.Space.large)
                .background(ConstructTheme.color(.background))
        } else {
            let available = !thread.loading && !thread.loadFailed
            VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
                if available && !state.modelReady {
                    InlineNotice(
                        text: state.modelReadFailed
                            ? "The model settings could not be read."
                            : "No model is set yet. Set one in Settings to chat.",
                        error: state.modelReadFailed,
                        actions: [.init(title: "Open Settings", run: openSettings)]
                    )
                }
                ChatComposerScreen(state: state, actions: actions, available: available)
            }
            .padding(.horizontal, ConstructTheme.Space.large)
            .padding(.top, ConstructTheme.Space.small)
            .background(ConstructTheme.color(.background))
        }
    }
}
