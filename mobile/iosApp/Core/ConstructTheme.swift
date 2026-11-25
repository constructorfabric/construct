import ConstructShared
import SwiftUI
import UIKit

/// The iPhone theme. The colours come from the tokens both apps share and follow the light or dark
/// appearance of the phone. The text sizes, radii and control sizes below are the agreed base look of
/// the phone apps. The navigation bar and the tab bar keep the heights and the title size the system
/// gives them.
enum ConstructTheme {
    /// A shared colour role, in the light or the dark values depending on the appearance.
    static func color(_ role: ColorRole) -> Color {
        Color(uiColor: UIColor { traits in
            argb(from: traits.userInterfaceStyle == .dark ? Colors.shared.dark : Colors.shared.light, role)
        })
    }

    /// The colour a shared role has in one theme, as a UIKit colour.
    static func argb(from roles: [ColorRole: KotlinLong], _ role: ColorRole) -> UIColor {
        guard let number = roles[role]?.int64Value else {
            return .magenta // A role missing from the tokens; a test makes sure none is.
        }
        return UIColor(
            red: CGFloat((number >> 16) & 0xFF) / 255,
            green: CGFloat((number >> 8) & 0xFF) / 255,
            blue: CGFloat(number & 0xFF) / 255,
            alpha: CGFloat((number >> 24) & 0xFF) / 255
        )
    }

    /// Paints the navigation bar and the tab bar with the surface colour and a hairline, in both
    /// appearances. Their heights, the title size and the selected state stay the system's.
    static func styleBars() {
        let surface = UIColor(color(.surface))
        let hairline = UIColor(color(.border))
        let muted = UIColor(color(.mutedForeground))

        let navigation = UINavigationBarAppearance()
        navigation.configureWithOpaqueBackground()
        navigation.backgroundColor = surface
        navigation.shadowColor = hairline
        UINavigationBar.appearance().standardAppearance = navigation
        UINavigationBar.appearance().scrollEdgeAppearance = navigation

        let tabs = UITabBarAppearance()
        tabs.configureWithOpaqueBackground()
        tabs.backgroundColor = surface
        tabs.shadowColor = hairline
        for item in [tabs.stackedLayoutAppearance, tabs.inlineLayoutAppearance, tabs.compactInlineLayoutAppearance] {
            item.normal.iconColor = muted
            item.normal.titleTextAttributes = [.foregroundColor: muted]
        }
        UITabBar.appearance().standardAppearance = tabs
        UITabBar.appearance().scrollEdgeAppearance = tabs
    }

    enum TextSize {
        static let body: CGFloat = 16
        static let secondary: CGFloat = 14
        static let caption: CGFloat = 12
        static let section: CGFloat = 20
        static let heading: CGFloat = 22
    }

    enum Radius {
        static let small: CGFloat = 8
        static let medium: CGFloat = 12
        static let large: CGFloat = 16
    }

    enum ControlSize {
        /// The smallest area a tap may land on.
        static let touch: CGFloat = 44
        static let icon: CGFloat = 24
        static let smallIcon: CGFloat = 16
        static let sendIcon: CGFloat = 20
        static let composerButton: CGFloat = 36
        static let composer: CGFloat = 52
        static let stroke: CGFloat = 1
        static let focusStroke: CGFloat = 2
        static let bubbleWidth: CGFloat = 0.82
        static let rowGap: CGFloat = 2
        /// The icon in the middle of an empty page.
        static let iconLarge: CGFloat = 40
        /// The dot of the connection line.
        static let dot: CGFloat = 8
    }

    /// The loading line of a web page, along the bottom of the address bar, and the look of a control that has
    /// nothing to do.
    enum Loading {
        static let height: CGFloat = 3
        /// The share of the line shown as soon as a page starts, so a start is seen before the first progress.
        static let firstShare: CGFloat = 0.1
        static let step: Double = 0.2
        static let dimmed: Double = 0.45
    }

    /// The spacing steps of the shared tokens, in points.
    enum Space {
        static let none: CGFloat = 0
        static let extraSmall = CGFloat(Spacing.shared.XS)
        static let small = CGFloat(Spacing.shared.S)
        static let medium = CGFloat(Spacing.shared.M)
        static let large = CGFloat(Spacing.shared.L)
        static let extraLarge = CGFloat(Spacing.shared.XL)
        static let extraExtraLarge = CGFloat(Spacing.shared.XXL)
    }
}
