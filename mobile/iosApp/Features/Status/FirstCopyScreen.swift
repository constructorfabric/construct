import ConstructShared
import SwiftUI

/// The loading screen after the first pairing: the three steps of the first copy with their counts and a bar that is
/// filled a third for each step. It has no tabs, and the app opens when the copy is whole. It shows the state it is
/// given and loads nothing.
struct FirstCopyScreen: View {
    let state: FirstCopy

    var body: some View {
        ScrollView {
            VStack(spacing: ConstructTheme.Space.large) {
                ProgressView()
                    .controlSize(.large)
                    .padding(.top, ConstructTheme.Space.extraExtraLarge)
                    .accessibilityHidden(true)
                Text("Copying from your computer")
                    .font(.system(size: ConstructTheme.TextSize.heading, weight: .semibold))
                    .multilineTextAlignment(.center)
                    .accessibilityAddTraits(.isHeader)
                Text("The first copy takes a moment. The app opens when it is done.")
                    .font(.system(size: ConstructTheme.TextSize.body))
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
                    .multilineTextAlignment(.center)
                CopyBar(fraction: Self.fraction(state.steps))
                    .padding(.top, ConstructTheme.Space.medium)
                Text("Step \(Self.current(state.steps)) of \(state.steps.count)")
                    .font(.system(size: ConstructTheme.TextSize.secondary))
                    .foregroundStyle(ConstructTheme.color(.mutedForeground))
                VStack(spacing: ConstructTheme.Space.none) {
                    Divider().overlay(ConstructTheme.color(.border))
                    ForEach(state.steps, id: \.step) { progress in
                        StepRow(progress: progress, active: progress.step == Self.activeStep(state.steps))
                        Divider().overlay(ConstructTheme.color(.border))
                    }
                }
                .padding(.top, ConstructTheme.Space.medium)
            }
            .padding(.horizontal, ConstructTheme.Space.large)
            .frame(maxWidth: .infinity)
        }
        .background(ConstructTheme.color(.background))
        .accessibilityIdentifier("first-copy")
    }

    /// A third of the bar for each step, and the part of the step that is copied, once its total is known.
    static func fraction(_ steps: [StepProgress]) -> Double {
        guard !steps.isEmpty else { return 0 }
        let parts = steps.map { progress -> Double in
            if progress.done { return 1 }
            guard let total = progress.total?.doubleValue, total > 0 else { return 0 }
            return min(Double(progress.copied) / total, 1)
        }
        return parts.reduce(0, +) / Double(steps.count)
    }

    /// The step being made: the first that is not done, or the last one when all are.
    static func current(_ steps: [StepProgress]) -> Int {
        (steps.firstIndex { !$0.done } ?? max(steps.count - 1, 0)) + 1
    }

    /// The steps are made one after the other, so the one under way is the first that is not done, even before it has
    /// reported a count.
    static func activeStep(_ steps: [StepProgress]) -> SyncStep? {
        steps.first { !$0.done }?.step
    }
}

private struct CopyBar: View {
    let fraction: Double

    var body: some View {
        Capsule()
            .fill(ConstructTheme.color(.border))
            .frame(height: ConstructTheme.ControlSize.bar)
            .overlay(alignment: .leading) {
                GeometryReader { size in
                    Capsule()
                        .fill(ConstructTheme.color(.primary))
                        .frame(width: size.size.width * fraction)
                }
            }
            .accessibilityElement()
            .accessibilityLabel("Copy progress")
            .accessibilityValue("\(Int((fraction * 100).rounded())) percent")
    }
}

private struct StepRow: View {
    let progress: StepProgress
    let active: Bool

    var body: some View {
        HStack(spacing: ConstructTheme.Space.medium) {
            marker
                .frame(width: ConstructTheme.ControlSize.smallIcon, height: ConstructTheme.ControlSize.smallIcon)
                .accessibilityHidden(true)
            Text(name)
                .font(.system(size: ConstructTheme.TextSize.body))
                .foregroundStyle(ConstructTheme.color(progress.done || active ? .foreground : .mutedForeground))
            Spacer(minLength: ConstructTheme.Space.medium)
            Text(count)
                .font(.system(size: ConstructTheme.TextSize.body))
                .foregroundStyle(ConstructTheme.color(.mutedForeground))
        }
        .padding(.vertical, ConstructTheme.Space.medium)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var marker: some View {
        if progress.done {
            FeatureIcon("NoticeCheck", size: ConstructTheme.ControlSize.smallIcon)
                .foregroundStyle(ConstructTheme.color(.success))
        } else if active {
            ProgressView().controlSize(.small)
        } else {
            FeatureIcon("NoticeCircle", size: ConstructTheme.ControlSize.smallIcon)
                .foregroundStyle(ConstructTheme.color(.mutedForeground))
        }
    }

    private var name: String {
        switch progress.step {
        case .memory: "Memory"
        case .chats: "Chats"
        case .dashboard: "Dashboard"
        }
    }

    private var count: String {
        let copied = Int(progress.copied)
        if progress.done { return Self.amount(copied, progress.step) }
        if let total = progress.total { return "\(copied.formatted()) of \(total.intValue.formatted())" }
        if active { return copied > 0 ? Self.amount(copied, progress.step) : "copying" }
        return "waiting"
    }

    private static func amount(_ number: Int, _ step: SyncStep) -> String {
        let unit: (one: String, many: String) =
            switch step {
            case .memory: ("fact", "facts")
            case .chats: ("chat", "chats")
            case .dashboard: ("number", "numbers")
            }
        return "\(number.formatted()) \(number == 1 ? unit.one : unit.many)"
    }
}
