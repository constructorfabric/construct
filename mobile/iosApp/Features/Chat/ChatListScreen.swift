import ConstructShared
import SwiftUI

struct ChatActions {
    let newChat: () -> Void
    let open: (ConversationSummary) -> Void
    let close: () -> Void
    let refresh: () -> Void
    let draft: () -> String
    let edit: (String) -> Void
    let send: (String) -> Void
    let stop: () -> Void
    let retry: () -> Void
    let delete: (String) -> Void
}

struct ChatListScreen: View {
    let state: ChatState
    let actions: ChatActions
    @State private var confirmDelete = false

    var body: some View {
        Group {
            if state.listFailed && state.conversations.isEmpty {
                SyncNoticePage {
                    InlineNotice(
                        text: "The conversations cannot be shown.",
                        error: true,
                        actions: [.init(title: "Try again", run: actions.refresh)]
                    )
                    .padding(ConstructTheme.Space.large)
                    .frame(maxHeight: .infinity, alignment: .top)
                }
            } else if state.listLoading && state.conversations.isEmpty {
                SyncNoticePage { ProgressView("Loading conversations") }
            } else if state.conversations.isEmpty {
                SyncNoticePage { empty }
            } else {
                list
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(ConstructTheme.color(.background))
        .toolbar {
            ToolbarItem(placement: .navigationBarLeading) {
                Menu {
                    Button("Delete all", role: .destructive) { confirmDelete = true }
                } label: {
                    FeatureIcon("ChatMore")
                }
                .accessibilityLabel("Chat actions")
                .disabled(state.answering || state.deleting || !state.conversations.contains { $0.origin == .phone })
            }
            ToolbarItem(placement: .navigationBarTrailing) {
                Button(action: actions.newChat) {
                    FeatureIcon("ChatNew")
                }
                .accessibilityLabel("New chat")
                .disabled(state.answering || state.deleting)
            }
        }
        .alert("Delete all conversations?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Yes, delete all", role: .destructive) { actions.delete(ChatKt.DELETE_ALL_CONVERSATIONS) }
        } message: {
            Text("Chats started on this phone will be deleted.")
        }
    }

    private var list: some View {
        List {
            SyncNoticeRow()
            if state.listFailed {
                InlineNotice(
                    text: "The conversations could not be refreshed.",
                    error: true,
                    actions: [.init(title: "Try again", run: actions.refresh)]
                )
                .listRowBackground(ConstructTheme.color(.background))
            }
            if state.deleteFailed && !state.threadOpen {
                InlineNotice(text: "The conversations could not be deleted.", error: true)
                    .listRowBackground(ConstructTheme.color(.background))
            }
            ForEach(state.conversations, id: \.id) { conversation in
                Button { actions.open(conversation) } label: {
                    ChatRow(
                        conversation: conversation,
                        answering: state.answering && state.thread?.id == conversation.id
                    )
                }
                .buttonStyle(.plain)
                .disabled(state.deleting || (state.answering && state.thread?.id != conversation.id))
                .listRowInsets(EdgeInsets(
                    top: ConstructTheme.Space.medium,
                    leading: ConstructTheme.Space.large,
                    bottom: ConstructTheme.Space.medium,
                    trailing: ConstructTheme.Space.large
                ))
                .listRowSeparatorTint(ConstructTheme.color(.border))
                .listRowBackground(ConstructTheme.color(.background))
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .refreshable { actions.refresh() }
    }

    @ViewBuilder
    private var empty: some View {
        if #available(iOS 17.0, *) {
            ContentUnavailableView {
                emptyLabel
            } description: {
                Text("Start a chat to see it here.")
            } actions: {
                emptyAction
            }
        } else {
            VStack(spacing: ConstructTheme.Space.medium) {
                emptyLabel
                Text("Start a chat to see it here.")
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
                emptyAction
            }
        }
    }

    private var emptyLabel: some View {
        Label {
            Text("No chats yet").font(.title2.weight(.semibold))
        } icon: {
            FeatureIcon("PageChat", size: ConstructTheme.ControlSize.iconLarge)
        }
    }

    private var emptyAction: some View {
        Button("New chat", action: actions.newChat)
            .buttonStyle(.bordered)
            .disabled(state.answering || state.deleting)
            .accessibilityIdentifier("new-chat-empty-state")
    }
}

private struct ChatRow: View {
    let conversation: ConversationSummary
    let answering: Bool

    var body: some View {
        let phone = conversation.origin == .phone
        VStack(alignment: .leading, spacing: ConstructTheme.ControlSize.rowGap) {
            HStack(alignment: .firstTextBaseline, spacing: ConstructTheme.Space.medium) {
                Text(conversation.title.isEmpty ? "Untitled" : conversation.title)
                    .font(.headline)
                    .foregroundStyle(ConstructTheme.color(.foreground))
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(Self.dateLabel(conversation.time))
                    .font(.caption)
                    .fixedSize()
            }
            HStack(alignment: .firstTextBaseline, spacing: ConstructTheme.Space.medium) {
                Group {
                    if answering {
                        HStack(spacing: ConstructTheme.Space.extraSmall) {
                            ProgressView().controlSize(.small).accessibilityHidden(true)
                            Text("Answering").fontWeight(.semibold)
                        }
                    } else {
                        Text(conversation.preview).lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: ConstructTheme.Space.extraSmall) {
                    FeatureIcon(phone ? "ChatPhone" : "ChatComputer", size: ConstructTheme.ControlSize.smallIcon)
                    Text(phone ? "This phone" : "Desktop")
                }
                .font(.caption)
                .fixedSize(horizontal: true, vertical: false)
            }
            .font(.system(size: ConstructTheme.TextSize.secondary))
        }
        .fixedSize(horizontal: false, vertical: true)
        .foregroundStyle(ConstructTheme.color(.mutedForeground))
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    private static func dateLabel(_ timestamp: String) -> String {
        let format = Date.ISO8601FormatStyle()
        let precise = Date.ISO8601FormatStyle(includingFractionalSeconds: true)
        guard let date = (try? format.parse(timestamp)) ?? (try? precise.parse(timestamp)) else { return "" }
        let calendar = Calendar.current
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        if let weekAgo = calendar.date(byAdding: .day, value: -6, to: .now), date >= weekAgo {
            return date.formatted(.dateTime.weekday(.abbreviated))
        }
        return date.formatted(.dateTime.month(.abbreviated).day())
    }
}
