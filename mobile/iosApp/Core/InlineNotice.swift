import SwiftUI

/// A quiet line of information or a problem, with the actions that can resolve it.
struct InlineNotice: View {
    struct Action {
        let title: String
        let run: () -> Void
    }

    let text: String
    var icon = "NoticeInfo"
    var error = false
    var emphasized = false
    /// Shows a spinner in place of the icon, for a notice about something still going on.
    var busy = false
    var actions: [Action] = []

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: ConstructTheme.Space.small) {
            if busy {
                ProgressView()
                    .controlSize(.mini)
                    .frame(width: ConstructTheme.ControlSize.smallIcon, height: ConstructTheme.ControlSize.smallIcon)
                    // A spinner has a baseline of its own: sit it on the text's line like the icons do.
                    .alignmentGuide(.firstTextBaseline) { $0[.bottom] }
                    .accessibilityHidden(true)
            } else {
                FeatureIcon(error ? "NoticeAlert" : icon, size: ConstructTheme.ControlSize.smallIcon)
                    .foregroundStyle(ConstructTheme.color(error ? .destructive : .mutedForeground))
            }
            VStack(alignment: .leading, spacing: 0) {
                Text(text)
                    .font(.system(size: ConstructTheme.TextSize.secondary, weight: emphasized ? .semibold : .regular))
                    .foregroundStyle(ConstructTheme.color(error ? .dangerForeground : .mutedForeground))
                    .fixedSize(horizontal: false, vertical: true)
                if !actions.isEmpty {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: ConstructTheme.Space.large) { buttons }
                            .fixedSize(horizontal: true, vertical: false)
                        VStack(alignment: .leading, spacing: 0) { buttons }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var buttons: some View {
        ForEach(actions, id: \.title) { action in
            Button(action.title, action: action.run)
                .font(.system(size: ConstructTheme.TextSize.secondary, weight: .semibold))
                .frame(minHeight: ConstructTheme.ControlSize.touch)
                .foregroundStyle(ConstructTheme.color(.linkForeground))
        }
    }
}

struct FeatureIcon: View {
    let name: String
    let size: CGFloat

    init(_ name: String, size: CGFloat = ConstructTheme.ControlSize.icon) {
        self.name = name
        self.size = size
    }

    var body: some View {
        Image(name)
            .resizable()
            .scaledToFit()
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}
