/// The five pages of the app, in the order of the tab bar. The icons are Lucide 1.33.0 (ISC licence),
/// in the asset catalog; the tab bar tints them.
enum Page: CaseIterable, Identifiable {
    case chat
    case memory
    case dashboard
    case glasshouse
    case settings

    var id: Self { self }

    var title: String {
        switch self {
        case .chat: "Chat"
        case .memory: "Memory"
        case .dashboard: "Dashboard"
        case .glasshouse: "Glasshouse"
        case .settings: "Settings"
        }
    }

    /// The name of the page's icon in the asset catalog.
    var icon: String {
        switch self {
        case .chat: "PageChat"
        case .memory: "PageMemory"
        case .dashboard: "PageDashboard"
        case .glasshouse: "PageGlasshouse"
        case .settings: "PageSettings"
        }
    }
}
