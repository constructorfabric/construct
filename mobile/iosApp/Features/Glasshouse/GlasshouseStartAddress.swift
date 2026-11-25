import Foundation

/// The address the Glasshouse page opens first. It is part of the build, set with `GLASSHOUSE_START_ADDRESS` in
/// `project.yml`, and not a setting of the app. There is no built-in one: the page says so and the person types one.
enum GlasshouseStartAddress {
    static let infoKey = "ConstructGlasshouseStartAddress"

    static func configured(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        bundle: Bundle = .main
    ) -> String? {
        #if DEBUG
        // The page tests start the app with the address of their test server.
        if let address = environment["CONSTRUCT_GLASSHOUSE_START_ADDRESS"] { return address }
        #endif
        // An empty setting is no address.
        return (bundle.object(forInfoDictionaryKey: infoKey) as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The app's own version, which the page's User-Agent carries as `Construct/<version>`.
    static func appVersion(bundle: Bundle = .main) -> String {
        let version = bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        return version.flatMap { $0.isEmpty || $0.contains(where: \.isWhitespace) ? nil : $0 } ?? "0"
    }
}
