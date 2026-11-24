import AVFoundation
import Combine
import ConstructShared
import SwiftUI
import UIKit

struct ComputerPage: View {
    @StateObject private var model: ComputerModel
    @State private var pairing: PairingMethod?

    init(
        status: AnyPublisher<ComputerStatus, Never>,
        connection: PhoneConnection,
        copy: CopyStore,
        activity: @escaping (PairResult) -> ComputerActivity,
        retry: @escaping () -> Void
    ) {
        _model = StateObject(wrappedValue: ComputerModel(
            status: status, connection: connection, copy: copy, activity: activity, retry: retry
        ))
    }

    var body: some View {
        ComputerScreen(
            status: model.status,
            operation: model.operation,
            open: { method in
                model.resetOperation()
                pairing = method
            },
            retry: model.retry,
            unpair: model.unpair
        )
        .navigationTitle("Computer")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.hidden, for: .tabBar)
        // Leaving while the unpairing removes the copied data would show the Memory or Chat page before the
        // removal ends, and a page that is open is read once.
        .navigationBarBackButtonHidden(model.operation.busy)
        .sheet(item: $pairing) { method in
            ComputerPairingSheet(method: method, model: model)
        }
    }
}

private struct ComputerPairingSheet: View {
    @State var method: PairingMethod
    @ObservedObject var model: ComputerModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @State private var cameraAccess: PairingCameraAccess = .checking

    var body: some View {
        NavigationStack {
            Group {
                switch method {
                case .manual:
                    ManualPairingScreen(operation: model.operation, pair: model.pairByCode)
                case .scan:
                    ScanPairingScreen(
                        access: cameraAccess,
                        active: scenePhase == .active,
                        operation: model.operation,
                        pair: model.pairByQr,
                        enterCode: {
                            model.resetOperation()
                            method = .manual
                        }
                    )
                }
            }
            .navigationTitle(method == .scan ? "Scan QR code" : "Enter address and code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(model.operation.busy)
                }
            }
        }
        .interactiveDismissDisabled(model.operation.busy)
        // Read the camera permission again each time the app comes back to the front: a person who went to
        // Settings to allow the camera returns to this sheet, and it must then show the scanner.
        .task(id: scenePhase) {
            guard method == .scan, scenePhase == .active else { return }
            guard AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back) != nil else {
                cameraAccess = .unavailable
                return
            }
            switch AVCaptureDevice.authorizationStatus(for: .video) {
            case .notDetermined:
                let granted = await AVCaptureDevice.requestAccess(for: .video)
                cameraAccess = granted ? .allowed : .denied
            case .authorized: cameraAccess = .allowed
            case .denied: cameraAccess = .denied
            default: cameraAccess = .unavailable
            }
        }
        .onChange(of: model.paired) { if $0 { dismiss() } }
        .onDisappear { model.resetOperation() }
    }
}

@MainActor
private final class ComputerModel: ObservableObject {
    @Published private(set) var status = ComputerStatus.checkingConnection
    @Published private(set) var operation = ComputerActivity.idle
    @Published private(set) var paired = false
    let retry: () -> Void
    private let connection: PhoneConnection
    private let copy: CopyStore
    private let activity: (PairResult) -> ComputerActivity
    private var observer: AnyCancellable?

    init(
        status: AnyPublisher<ComputerStatus, Never>,
        connection: PhoneConnection,
        copy: CopyStore,
        activity: @escaping (PairResult) -> ComputerActivity,
        retry: @escaping () -> Void
    ) {
        self.connection = connection
        self.copy = copy
        self.activity = activity
        self.retry = retry
        observer = status.sink { [weak self] in self?.status = $0 }
    }

    func pairByCode(address: String, code: String) {
        pair { [connection] in
            try await connection.pairByCode(address: address, code: code, name: UIDevice.current.model)
        }
    }

    func pairByQr(_ payload: String) {
        pair { [connection] in
            try await connection.pairByQr(payload: payload, name: UIDevice.current.model)
        }
    }

    private func pair(_ action: @escaping () async throws -> PairResult) {
        guard !operation.busy else { return }
        operation = .working
        Task {
            do {
                let result = try await action()
                var outcome = activity(result)
                if outcome.needsNetworkPermission {
                    outcome.message = ComputerStatus.allowLocalNetwork
                }
                operation = outcome
                paired = outcome.paired
            } catch {
                // The shared code reports every failure as a PairResult, so only a cancelled task lands here.
                // Swift still needs the catch, because every Kotlin suspend function is `async throws`.
                operation = .failed("Pairing could not be completed. Try again.")
            }
        }
    }

    func unpair() {
        guard !operation.busy else { return }
        operation = .working
        Task {
            do {
                _ = try await ComputerPairingKt.unpairComputer(connection: connection, copy: copy)
                operation = .idle
            } catch {
                operation = .failed("The computer could not be unpaired. Try again.")
            }
        }
    }

    func resetOperation() {
        guard !operation.busy else { return }
        paired = false
        operation = .idle
    }
}
