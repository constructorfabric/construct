import ConstructShared
import SwiftUI

struct ChatMessagesScreen: View {
    let state: ChatState
    let thread: ChatThread
    let actions: ChatActions
    let openSettings: () -> Void

    var body: some View {
        let newestTool = thread.messages.lastIndex { $0.role == .tool }
        let visibleReply = Self.visibleReply(thread.reply, modelReady: state.modelReady)
        GeometryReader { space in
            ScrollViewReader { reader in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: ConstructTheme.Space.large) {
                        ForEach(Array(thread.messages.enumerated()), id: \.offset) { index, message in
                            ChatMessageRow(
                                message: message,
                                bubbleWidth: (space.size.width - 2 * ConstructTheme.Space.large)
                                    * ConstructTheme.ControlSize.bubbleWidth,
                                emphasized: state.answering && index == newestTool
                            )
                        }
                        if let reply = visibleReply {
                            replyNotice(reply)
                        }
                        if state.deleteFailed {
                            InlineNotice(text: "The conversation could not be deleted.", error: true)
                        }
                        Color.clear.frame(height: ConstructTheme.Space.none).id("end")
                    }
                    .padding(ConstructTheme.Space.large)
                }
                .scrollDismissesKeyboard(.interactively)
                .onAppear { reader.scrollTo("end", anchor: .bottom) }
                .onChange(of: thread.messages) { _ in reader.scrollTo("end", anchor: .bottom) }
                .onChange(of: visibleReply.map { onEnum(of: $0) }) { _ in
                    reader.scrollTo("end", anchor: .bottom)
                }
            }
        }
        .textSelection(.enabled)
    }

    static func visibleReply(_ reply: ChatReply?, modelReady: Bool) -> ChatReply? {
        let isMissing = (reply as? ChatReplyFailed)?.modelMissing == true
        return isMissing && !modelReady ? nil : reply
    }

    @ViewBuilder
    private func replyNotice(_ reply: ChatReply) -> some View {
        let retry = InlineNotice.Action(title: "Try again", run: actions.retry)
        if let failure = reply as? ChatReplyFailed {
            let settings = InlineNotice.Action(title: "Open Settings", run: openSettings)
            InlineNotice(
                text: failure.message,
                error: true,
                actions: (failure.hasSettingsFix ? [settings] : []) + (state.modelReady ? [retry] : [])
            )
        } else {
            InlineNotice(
                text: "Stopped. The part that was written is kept.",
                actions: state.modelReady ? [retry] : []
            )
        }
    }
}

private struct ChatMessageRow: View {
    let message: ChatMessage
    let bubbleWidth: CGFloat
    let emphasized: Bool

    var body: some View {
        switch message.role {
        case .user:
            Text(message.text)
                .padding(.horizontal, ConstructTheme.Space.large)
                .padding(.vertical, ConstructTheme.Space.medium)
                .background {
                    RoundedRectangle(cornerRadius: ConstructTheme.Radius.large).strokeBorder(
                        ConstructTheme.color(.borderStrong),
                        lineWidth: ConstructTheme.ControlSize.stroke
                    )
                }
                .frame(maxWidth: bubbleWidth, alignment: .trailing)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .foregroundStyle(ConstructTheme.color(.foreground))
        case .assistant:
            ChatAnswer(text: message.text)
                .foregroundStyle(ConstructTheme.color(.foreground))
        case .tool:
            InlineNotice(text: message.text, icon: "ChatTool", emphasized: emphasized)
        }
    }
}

/// Render the agreed inline formatting and simple lists as text arrives.
private struct ChatAnswer: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
            ForEach(Array(text.components(separatedBy: "\n").enumerated()), id: \.offset) { _, line in
                if let (marker, content) = listItem(line) {
                    HStack(alignment: .firstTextBaseline, spacing: ConstructTheme.Space.small) {
                        Text(marker)
                            .frame(minWidth: ConstructTheme.ControlSize.smallIcon, alignment: .trailing)
                            .accessibilityHidden(true)
                        Text(ChatAnswerMarkdown.inline(content))
                    }
                } else {
                    Text(ChatAnswerMarkdown.inline(line))
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .environment(\.openURL, OpenURLAction { url in
            ChatAnswerMarkdown.isWebLink(url) ? .systemAction : .discarded
        })
        .accessibilityElement(children: .combine)
    }

    private func listItem(_ line: String) -> (String, String)? {
        let parts = line.trimmingCharacters(in: .whitespaces).split(separator: " ", maxSplits: 1)
        guard parts.count == 2, let marker = parts.first else { return nil }
        if ["-", "*", "+"].contains(marker) { return ("•", String(parts[1])) }
        if marker.hasSuffix("."), Int(marker.dropLast()) != nil { return (String(marker), String(parts[1])) }
        return nil
    }
}

enum ChatAnswerMarkdown {
    static func inline(_ line: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        guard let rendered = try? AttributedString(markdown: line, options: options) else {
            return AttributedString(line)
        }
        return rendered.runs.reduce(into: AttributedString()) { result, run in
            var segment = AttributedString(rendered[run.range])
            if let url = run.link, !isWebLink(url) {
                segment.link = nil
            }
            result += segment
        }
    }

    static func isWebLink(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), let host = url.host, !host.isEmpty else { return false }
        return scheme == "http" || scheme == "https"
    }
}
