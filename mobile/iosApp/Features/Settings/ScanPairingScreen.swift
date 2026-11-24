import AVFoundation
import SwiftUI
import UIKit

enum PairingCameraAccess {
    case checking
    case allowed
    case denied
    case unavailable
}

struct ScanPairingScreen: View {
    let access: PairingCameraAccess
    let active: Bool
    let operation: ComputerOperation
    let pair: (String) -> Void
    let enterCode: () -> Void
    @State private var unavailable = false
    @State private var scanning = true

    var body: some View {
        VStack(alignment: .leading, spacing: ConstructTheme.Space.large) {
            if access == .allowed, !unavailable {
                Text("Point the camera at the pairing QR code shown by Construct on your computer.")
                if scanning, active {
                    PairingCamera(
                        onRead: { payload in
                            scanning = false
                            pair(payload)
                        },
                        onUnavailable: { unavailable = true }
                    )
                    .clipShape(RoundedRectangle(cornerRadius: ConstructTheme.Radius.medium))
                    .accessibilityHidden(true)
                }
                ComputerFeedback(operation: operation)
                if !scanning, operation.message != nil {
                    Button("Scan again") { scanning = true }
                        .buttonStyle(.bordered)
                }
            } else if access == .checking {
                ProgressView("Opening the camera")
            } else if unavailable || access == .unavailable {
                Text("The camera is unavailable.")
                manualEntry
            } else {
                Text("Allow camera access in Settings to scan the code, or use the address and code instead.")
                OpenAppSettingsButton()
                manualEntry
            }
            Spacer(minLength: ConstructTheme.Space.none)
        }
        .padding(ConstructTheme.Space.large)
        .frame(maxWidth: .infinity, alignment: .leading)
        .foregroundStyle(ConstructTheme.color(.foreground))
        .background(ConstructTheme.color(.background))
    }

    private var manualEntry: some View {
        Button("Enter address and code", action: enterCode)
            .buttonStyle(.bordered)
            .accessibilityIdentifier("camera-manual-entry")
    }
}

private struct PairingCamera: UIViewRepresentable {
    let onRead: (String) -> Void
    let onUnavailable: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onRead: onRead, onUnavailable: onUnavailable)
    }

    func makeUIView(context: Context) -> Preview {
        let view = Preview()
        view.previewLayer.videoGravity = .resizeAspectFill
        view.previewLayer.session = context.coordinator.session
        context.coordinator.start()
        return view
    }

    func updateUIView(_ uiView: Preview, context: Context) {}

    static func dismantleUIView(_ uiView: Preview, coordinator: Coordinator) {
        coordinator.stop()
    }

    final class Preview: UIView {
        override static var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

        var previewLayer: AVCaptureVideoPreviewLayer {
            // UIView creates its backing layer from layerClass.
            // swiftlint:disable:next force_cast
            layer as! AVCaptureVideoPreviewLayer
        }
    }

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        let session = AVCaptureSession()
        private static let queue = DispatchQueue(label: "Construct pairing camera")
        private var listening = true
        private let onRead: (String) -> Void
        private let onUnavailable: () -> Void
        private var runtimeError: NSObjectProtocol?

        init(onRead: @escaping (String) -> Void, onUnavailable: @escaping () -> Void) {
            self.onRead = onRead
            self.onUnavailable = onUnavailable
        }

        func start() {
            runtimeError = NotificationCenter.default.addObserver(
                forName: AVCaptureSession.runtimeErrorNotification, object: session, queue: .main
            ) { [weak self] _ in
                self?.cameraUnavailable()
            }
            Self.queue.async {
                if self.configure() {
                    self.session.startRunning()
                }
                if !self.session.isRunning {
                    DispatchQueue.main.async { self.cameraUnavailable() }
                }
            }
        }

        func stop() {
            guard listening else { return }
            listening = false
            if let runtimeError { NotificationCenter.default.removeObserver(runtimeError) }
            Self.queue.async { [session] in session.stopRunning() }
        }

        private func cameraUnavailable() {
            guard listening else { return }
            stop()
            onUnavailable()
        }

        private func configure() -> Bool {
            guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
                  let input = try? AVCaptureDeviceInput(device: device) else { return false }
            let output = AVCaptureMetadataOutput()
            session.beginConfiguration()
            defer { session.commitConfiguration() }
            guard session.canAddInput(input) else { return false }
            session.addInput(input)
            guard session.canAddOutput(output) else { return false }
            session.addOutput(output)
            guard output.availableMetadataObjectTypes.contains(.qr) else { return false }
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
            return true
        }

        func metadataOutput(
            _ output: AVCaptureMetadataOutput,
            didOutput metadataObjects: [AVMetadataObject],
            from connection: AVCaptureConnection
        ) {
            guard listening,
                  let code = metadataObjects.compactMap({ ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue })
                    .first else { return }
            stop()
            onRead(code)
        }
    }
}
