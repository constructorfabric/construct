import ConstructShared
import SwiftUI

struct SettingsActions {
    let edit: (SettingsField, String) -> Void
    let save: () -> Void
    let test: () -> Void
    let remove: () -> Void
    let reload: () -> Void
}

struct SettingsScreen: View {
    @ObservedObject var state: FlowState<SettingsState>
    let actions: SettingsActions
    @FocusState private var focus: SettingsField?

    var body: some View {
        Group {
            if state.value.loading {
                ProgressView("Loading the settings")
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: ConstructTheme.Space.large) {
                        if let reason = state.value.unavailable {
                            InlineNotice(
                                text: "The settings are not available: \(reason)",
                                error: true,
                                actions: [.init(title: "Try again", run: actions.reload)]
                            )
                        } else {
                            form
                        }
                    }
                    .padding(ConstructTheme.Space.large)
                }
                .scrollDismissesKeyboard(.interactively)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(ConstructTheme.color(.background))
    }

    private var form: some View {
        let current = state.value
        return VStack(alignment: .leading, spacing: ConstructTheme.Space.medium) {
            Text("Model")
                .font(.system(size: ConstructTheme.TextSize.section, weight: .semibold))
                .accessibilityAddTraits(.isHeader)
            field("Address", .address, missing: current.addressMissing) {
                TextField("https://", text: binding(.address, \.address))
                    .keyboardType(.URL)
            }
            field("Model name", .modelName, missing: current.modelMissing) {
                TextField("Model name", text: binding(.modelName, \.model))
            }
            field("API key", .key) {
                SecureField(current.keyIsSet ? "••••••••" : "API key", text: binding(.key, \.keyEntry))
            }
            if current.keyIsSet {
                Text("A key is saved. Leave this empty to keep it.")
                    .font(.system(size: ConstructTheme.TextSize.caption))
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
            }
            buttons(current)
            if current.saved {
                InlineNotice(text: "Saved.", icon: "NoticeCheck")
            }
            if let failure = current.failure {
                InlineNotice(text: failure, error: true)
            }
            if let check = current.check as? SettingsCheckShown {
                SettingsChecks(check: check)
            }
        }
        .foregroundStyle(ConstructTheme.color(.foreground))
    }

    private func field<Input: View>(
        _ title: String,
        _ field: SettingsField,
        missing: Bool = false,
        @ViewBuilder input: () -> Input
    ) -> some View {
        let border = missing ? ColorRole.destructive : focus == field ? .ring : .input
        let shape = RoundedRectangle(cornerRadius: ConstructTheme.Radius.medium)
        return VStack(alignment: .leading, spacing: ConstructTheme.Space.extraSmall) {
            Text(title)
                .font(.system(size: ConstructTheme.TextSize.secondary, weight: .medium))
                .foregroundStyle(ConstructTheme.color(missing ? .dangerForeground : .foreground))
                .accessibilityHidden(true)
            input()
                .accessibilityLabel(title)
                .focused($focus, equals: field)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.done)
                .onSubmit { focus = nil }
                .padding(.horizontal, ConstructTheme.Space.medium)
                .frame(minHeight: ConstructTheme.ControlSize.touch)
                .background(ConstructTheme.color(.card), in: shape)
                .overlay {
                    let stroke = focus == field
                        ? ConstructTheme.ControlSize.focusStroke : ConstructTheme.ControlSize.stroke
                    shape.strokeBorder(ConstructTheme.color(border), lineWidth: stroke)
                }
            if missing {
                Text("This field is required")
                    .font(.system(size: ConstructTheme.TextSize.caption))
                    .foregroundStyle(ConstructTheme.color(.dangerForeground))
            }
        }
    }

    private func binding(_ field: SettingsField, _ keyPath: KeyPath<SettingsState, String>) -> Binding<String> {
        Binding(get: { state.value[keyPath: keyPath] }, set: { actions.edit(field, $0) })
    }

    private func buttons(_ current: SettingsState) -> some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: ConstructTheme.Space.small) { formActions(current) }
                .fixedSize(horizontal: true, vertical: false)
            VStack(alignment: .leading, spacing: ConstructTheme.Space.small) { formActions(current) }
        }
    }

    @ViewBuilder
    private func formActions(_ current: SettingsState) -> some View {
        Button("Save") { run(actions.save) }
            .buttonStyle(SettingsButton(fill: .primary, ink: .primaryForeground))
        Button {
            run(actions.test)
        } label: {
            HStack {
                if current.testing { ProgressView().controlSize(.small).accessibilityHidden(true) }
                Text("Test")
            }
        }
        .buttonStyle(SettingsButton(fill: .secondary, ink: .secondaryForeground))
        .disabled(!current.canTest)
        if current.keyIsSet {
            Button("Remove key", role: .destructive) { run(actions.remove) }
                .buttonStyle(SettingsButton(fill: .dangerSoft, ink: .dangerForeground))
        }
    }

    private func run(_ action: () -> Void) {
        focus = nil
        action()
    }
}

private struct SettingsButton: ButtonStyle {
    let fill: ColorRole
    let ink: ColorRole
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: ConstructTheme.TextSize.secondary, weight: .semibold))
            .padding(.horizontal, ConstructTheme.Space.large)
            .frame(minHeight: ConstructTheme.ControlSize.touch)
            .foregroundStyle(ConstructTheme.color(ink))
            .background(ConstructTheme.color(fill), in: RoundedRectangle(cornerRadius: ConstructTheme.Radius.medium))
            .opacity(configuration.isPressed || !enabled ? 0.55 : 1)
    }
}

private struct SettingsChecks: View {
    let check: SettingsCheckShown

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.small) {
            Text("Checks of the model")
                .font(.system(size: ConstructTheme.TextSize.secondary, weight: .semibold))
            line("Answers", check.answers)
            line("Uses tools", check.tools)
        }
    }

    @ViewBuilder
    private func line(_ title: String, _ line: CheckLine) -> some View {
        switch line.status {
        case .checking:
            HStack(spacing: ConstructTheme.Space.small) {
                ProgressView().controlSize(.small).accessibilityHidden(true)
                Text("\(title): checking")
                    .font(.system(size: ConstructTheme.TextSize.secondary))
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
            }
        case .works:
            InlineNotice(text: "\(title): works", icon: "NoticeCheck")
        case .failed:
            InlineNotice(text: "\(title): \(line.reason ?? "failed")", error: true)
        case .skipped:
            InlineNotice(text: "\(title): skipped", icon: "NoticeCircle")
        }
    }
}
