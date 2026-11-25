import SwiftUI
import UIKit

/// The browser's address bar: one rounded bar with the lock, the host and reload. Tapped, it shows the whole address
/// with a caret and "Cancel" beside it, and the keyboard's Go key opens what was typed. While a page loads, a thin
/// progress line runs along the bottom of the bar.
struct GlasshouseAddressBar: View {
    let address: String
    let secure: Bool
    let loading: Bool
    let progress: Double
    let openAddress: (String) -> Bool
    let edited: () -> Void
    let reload: () -> Void

    @State private var editing = false
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: ConstructTheme.Space.medium) {
            bar
            if editing {
                Button("Cancel", action: cancel)
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundStyle(ConstructTheme.color(.linkForeground))
                    .frame(minHeight: ConstructTheme.ControlSize.touch)
                    .accessibilityIdentifier("glasshouse-cancel")
            }
        }
    }

    private var bar: some View {
        ZStack {
            if editing { field } else { shown }
        }
        .frame(minHeight: ConstructTheme.ControlSize.touch)
        .frame(maxWidth: .infinity)
        .background(ConstructTheme.color(.selectionSubtle), in: Capsule())
        .overlay(alignment: .bottom) { progressLine }
        .clipShape(Capsule())
    }

    /// The lock (for an `https` page that opened), the host in the middle, and reload.
    private var shown: some View {
        HStack(spacing: ConstructTheme.Space.none) {
            FeatureIcon("GlasshouseLock", size: ConstructTheme.ControlSize.smallIcon)
                .opacity(secure ? 1 : 0)
                .frame(width: ConstructTheme.ControlSize.touch, height: ConstructTheme.ControlSize.touch)
            Button(action: startEditing) {
                Text(address.isEmpty ? "Address" : GlasshouseHost.display(address))
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundStyle(ConstructTheme.color(address.isEmpty ? .mutedForeground : .foreground))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, minHeight: ConstructTheme.ControlSize.touch)
                    .contentShape(Rectangle())
            }
            .accessibilityIdentifier("glasshouse-host")
            Button(action: reload) {
                FeatureIcon("GlasshouseReload", size: ConstructTheme.ControlSize.smallIcon)
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
                    .frame(width: ConstructTheme.ControlSize.touch, height: ConstructTheme.ControlSize.touch)
            }
            .accessibilityLabel("Reload page")
            .accessibilityIdentifier("glasshouse-reload")
        }
    }

    private var field: some View {
        TextField("Address", text: $draft)
            .focused($focused)
            .keyboardType(.URL)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .submitLabel(.go)
            .font(.system(size: ConstructTheme.TextSize.body))
            .foregroundStyle(ConstructTheme.color(.foreground))
            .padding(.horizontal, ConstructTheme.Space.large)
            .frame(minHeight: ConstructTheme.ControlSize.touch)
            .onSubmit(submit)
            .onChange(of: draft) { _ in edited() }
            .onReceive(NotificationCenter.default.publisher(for: UITextField.textDidBeginEditingNotification)) { note in
                // As in a browser, the first thing typed replaces the whole address.
                let field = note.object as? UITextField
                DispatchQueue.main.async { field?.selectAll(nil) }
            }
            .accessibilityIdentifier("glasshouse-address")
    }

    @ViewBuilder
    private var progressLine: some View {
        if loading {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Rectangle().fill(ConstructTheme.color(.border))
                    Rectangle()
                        .fill(ConstructTheme.color(.primary))
                        .frame(width: geometry.size.width * min(max(progress, ConstructTheme.Loading.firstShare), 1))
                }
            }
            .frame(height: ConstructTheme.Loading.height)
            .animation(.linear(duration: ConstructTheme.Loading.step), value: progress)
            .accessibilityHidden(true)
        }
    }

    private func startEditing() {
        draft = address
        editing = true
        DispatchQueue.main.async { focused = true }
    }

    /// Goes to what was typed. A refused address stays in the bar, with the message under it.
    private func submit() {
        if openAddress(draft) {
            editing = false
            focused = false
        }
    }

    private func cancel() {
        editing = false
        focused = false
        edited()
    }
}
