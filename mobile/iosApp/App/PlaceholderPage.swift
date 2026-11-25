import SwiftUI

/// Stands in for a page that has no feature yet: its icon and title.
struct PlaceholderPage: View {
    let page: Page

    var body: some View {
        SyncNoticePage {
            VStack(spacing: ConstructTheme.Space.medium) {
                Image(page.icon)
                    .resizable()
                    .scaledToFit()
                    .frame(width: ConstructTheme.ControlSize.iconLarge, height: ConstructTheme.ControlSize.iconLarge)
                Text(page.title)
            }
            .foregroundColor(ConstructTheme.color(.mutedForeground))
            .accessibilityElement(children: .combine)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(ConstructTheme.color(.background))
    }
}
