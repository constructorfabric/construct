@testable import Construct
import ConstructShared
import UIKit
import XCTest

final class ConstructThemeTests: XCTestCase {
    func testEveryRoleHasAValueInBothThemes() {
        for role in ColorRole.allCases {
            XCTAssertNotNil(Colors.shared.light[role], "\(role) has no light value")
            XCTAssertNotNil(Colors.shared.dark[role], "\(role) has no dark value")
        }
    }

    func testTheColourComesFromTheSharedNumber() {
        // Literals on purpose: they check the decoding of the number on their own, whatever `Colors` holds.
        // The light background is 0xFFF1F5F9, the dark one 0xFF0F172A.
        assertColor(ConstructTheme.argb(from: Colors.shared.light, .background), red: 0xF1, green: 0xF5, blue: 0xF9)
        assertColor(ConstructTheme.argb(from: Colors.shared.dark, .background), red: 0x0F, green: 0x17, blue: 0x2A)
    }

    func testTheColourFollowsTheAppearanceOfThePhone() {
        let background = UIColor(ConstructTheme.color(.background))
        let light = background.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light))
        let dark = background.resolvedColor(with: UITraitCollection(userInterfaceStyle: .dark))
        assertColor(light, red: 0xF1, green: 0xF5, blue: 0xF9)
        assertColor(dark, red: 0x0F, green: 0x17, blue: 0x2A)
    }

    private func assertColor(
        _ color: UIColor,
        red: Int,
        green: Int,
        blue: Int,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        var parts = (red: CGFloat(0), green: CGFloat(0), blue: CGFloat(0), alpha: CGFloat(0))
        color.getRed(&parts.red, green: &parts.green, blue: &parts.blue, alpha: &parts.alpha)
        XCTAssertEqual(parts.red * 255, CGFloat(red), accuracy: 0.5, file: file, line: line)
        XCTAssertEqual(parts.green * 255, CGFloat(green), accuracy: 0.5, file: file, line: line)
        XCTAssertEqual(parts.blue * 255, CGFloat(blue), accuracy: 0.5, file: file, line: line)
        XCTAssertEqual(parts.alpha, 1, accuracy: 0.001, file: file, line: line)
    }
}
