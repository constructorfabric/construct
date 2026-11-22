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

    enum TextSize {
        static let body: CGFloat = 16
        static let secondary: CGFloat = 14
        static let caption: CGFloat = 12
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
        /// The icon in the middle of an empty page.
        static let iconLarge: CGFloat = 40
    }

    /// The spacing steps of the shared tokens, in points.
    enum Space {
        static let extraSmall = CGFloat(Spacing.shared.XS)
        static let small = CGFloat(Spacing.shared.S)
        static let medium = CGFloat(Spacing.shared.M)
        static let large = CGFloat(Spacing.shared.L)
        static let extraLarge = CGFloat(Spacing.shared.XL)
        static let extraExtraLarge = CGFloat(Spacing.shared.XXL)
    }
}
