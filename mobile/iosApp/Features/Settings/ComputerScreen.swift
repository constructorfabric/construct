import ConstructShared
import SwiftUI
import UIKit

struct ComputerStatus {
    let label: String
    let notice: String
    let endpoint: String?
    /// When the copy on the phone was last brought up to date, or nil if it never was.
    let lastSynced: Date?
    /// Whether a sync that the person waits for is running.
    let syncing: Bool
    let checking: Bool
    let unpaired: Bool
    let connected: Bool
    let canRetry: Bool
    let needsLocalNetwork: Bool
    let dot: ColorRole

    /// What the page says when Local Network access is refused. The Android page asks for the permission
    /// itself. An iPhone cannot be asked ahead of use and the app cannot read the answer, so the text
    /// sends the person to Settings, on the page and after a failed pairing alike.
    static let allowLocalNetwork = "Allow Local Network access for Construct in Settings to connect to your computer."

    static let checkingConnection = Self(
        label: "Checking connection…", notice: "", endpoint: nil, lastSynced: nil, syncing: false,
        checking: true, unpaired: false, connected: false, canRetry: false, needsLocalNetwork: false,
        dot: .borderStrong
    )
}

/// What the pairing and unpairing screens show about the operation in progress or just finished.
struct ComputerActivity {
    var busy = false
    var paired = false
    var message: String?
    var needsNetworkPermission = false
    var needsFreshCode = false

    /// Nothing is running and nothing is shown.
    static let idle = Self()
    /// A pairing or an unpairing is running.
    static let working = Self(busy: true)

    static func failed(_ message: String) -> Self {
        Self(message: message)
    }
}

enum PairingMethod: String, Identifiable {
    case manual
    case scan

    var id: Self { self }
}

struct ComputerSettingsRow: View {
    let status: ComputerStatus

    var body: some View {
        HStack(spacing: ConstructTheme.Space.small) {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
                Text("Computer")
                    .foregroundStyle(ConstructTheme.color(.foreground))
                if let endpoint = status.endpoint {
                    Text(endpoint)
                        .font(.system(size: ConstructTheme.TextSize.caption))
                        .foregroundStyle(ConstructTheme.color(.mutedForeground))
                }
            }
            Spacer()
            StatusDot(role: status.dot)
            Text(status.label)
                .font(.system(size: ConstructTheme.TextSize.secondary))
                .foregroundStyle(ConstructTheme.color(.mutedForeground))
        }
        .accessibilityElement(children: .combine)
    }
}

struct ComputerScreen: View {
    let status: ComputerStatus
    let operation: ComputerActivity
    let open: (PairingMethod) -> Void
    let retry: () -> Void
    let syncNow: () -> Void
    let unpair: () -> Void
    @State private var confirming = false
    @State private var replacing = false

    var body: some View {
        GeometryReader { size in
            ScrollView {
                VStack(alignment: status.unpaired ? .center : .leading, spacing: ConstructTheme.Space.large) {
                    content
                    ComputerFeedback(operation: operation)
                }
                .padding(ConstructTheme.Space.large)
                .frame(maxWidth: .infinity, alignment: status.unpaired ? .center : .leading)
                .frame(minHeight: size.size.height, alignment: status.unpaired ? .center : .top)
            }
        }
        .foregroundStyle(ConstructTheme.color(.foreground))
        .background(ConstructTheme.color(.background))
        .alert("Unpair this computer?", isPresented: $confirming) {
            Button("Cancel", role: .cancel) {}
            Button("Unpair", role: .destructive, action: unpair)
        } message: {
            Text("This removes the computer’s copied data. Chats started on this phone stay.")
        }
        .onChange(of: status.unpaired) { _ in replacing = false }
        .onChange(of: status.connected) { _ in replacing = false }
    }

    @ViewBuilder
    private var content: some View {
        if status.checking {
            ProgressView(status.label)
        } else if status.unpaired {
            Image(systemName: "link.badge.plus")
                .font(.system(size: ConstructTheme.ControlSize.iconLarge))
                .foregroundStyle(ConstructTheme.color(.mutedForeground))
                .accessibilityHidden(true)
            Text("Not paired yet")
                .font(.system(size: ConstructTheme.TextSize.heading, weight: .semibold))
                .accessibilityAddTraits(.isHeader)
            PairingChoices(open: open, enabled: !operation.busy)
                .multilineTextAlignment(.center)
        } else {
            if !status.connected {
                InlineNotice(text: status.notice)
            }
            if status.needsLocalNetwork {
                OpenAppSettingsButton()
            }
            details
            if status.connected {
                Button(action: syncNow) {
                    HStack(spacing: ConstructTheme.Space.small) {
                        if status.syncing { ProgressView().accessibilityHidden(true) }
                        Text(status.syncing ? "Syncing…" : "Sync now")
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .disabled(operation.busy || status.syncing)
            }
            if status.canRetry {
                Button(action: retry) {
                    Text("Try again").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .disabled(operation.busy)
            }
            if !status.connected {
                Button("Pair again") { replacing.toggle() }
                    .disabled(operation.busy)
                if replacing {
                    PairingChoices(open: open, enabled: !operation.busy)
                }
            }
            Button("Unpair", role: .destructive) { confirming = true }
                .foregroundStyle(ConstructTheme.color(.dangerForeground))
                .disabled(operation.busy)
        }
    }

    private var details: some View {
        VStack(spacing: ConstructTheme.Space.none) {
            if let endpoint = status.endpoint {
                ComputerDetail(title: "Paired computer", value: endpoint)
            }
            ComputerDetail(
                title: "Connection",
                value: status.label,
                detail: status.lastSynced.map(SyncText.lastSync),
                dot: status.dot
            )
            ComputerDetail(
                title: "Agent",
                value: status.connected ? "Running" : "Unknown",
                detail: status.connected ? nil : "Known again when the computer answers.",
                dot: status.connected ? .success : .borderStrong
            )
        }
    }
}

private struct PairingChoices: View {
    let open: (PairingMethod) -> Void
    let enabled: Bool

    var body: some View {
        Text("On your computer, open Settings and choose Pair a phone.")
            .font(.system(size: ConstructTheme.TextSize.secondary))
            .foregroundStyle(ConstructTheme.color(.mutedForeground))
        Button {
            open(.scan)
        } label: {
            Text("Scan QR code")
                .foregroundStyle(ConstructTheme.color(.primaryForeground))
        }
        .buttonStyle(.borderedProminent)
        .tint(ConstructTheme.color(.primary))
        .controlSize(.large)
        .disabled(!enabled)
        Button("Enter address and code") { open(.manual) }
            .buttonStyle(.bordered)
            .controlSize(.large)
            .disabled(!enabled)
    }
}

private struct ComputerDetail: View {
    let title: String
    let value: String
    var detail: String?
    var dot: ColorRole?

    var body: some View {
        HStack(alignment: .top, spacing: ConstructTheme.Space.medium) {
            VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
                Text(title).font(.system(size: ConstructTheme.TextSize.secondary))
                if let detail {
                    Text(detail).font(.system(size: ConstructTheme.TextSize.caption))
                }
            }
            .foregroundStyle(ConstructTheme.color(.mutedForeground))
            Spacer()
            HStack(spacing: ConstructTheme.Space.small) {
                if let dot { StatusDot(role: dot) }
                Text(value)
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .multilineTextAlignment(.trailing)
            }
        }
        .padding(.vertical, ConstructTheme.Space.medium)
        Divider().overlay(ConstructTheme.color(.border))
    }
}

private struct StatusDot: View {
    let role: ColorRole

    var body: some View {
        Circle()
            .fill(ConstructTheme.color(role))
            .frame(width: ConstructTheme.ControlSize.dot, height: ConstructTheme.ControlSize.dot)
            .accessibilityHidden(true)
    }
}

struct ComputerFeedback: View {
    let operation: ComputerActivity

    var body: some View {
        if operation.busy {
            ProgressView()
        }
        if let message = operation.message {
            InlineNotice(text: message, error: true)
        }
        if operation.needsNetworkPermission {
            OpenAppSettingsButton()
        }
    }
}

struct OpenAppSettingsButton: View {
    @Environment(\.openURL) private var openURL

    var body: some View {
        Button("Open Settings") {
            if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
        }
        .buttonStyle(.bordered)
    }
}
