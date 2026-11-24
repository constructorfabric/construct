import ConstructShared
import SwiftUI

struct ManualPairingScreen: View {
    let operation: ComputerOperation
    let pair: (String, String) -> Void
    @State private var address = ""
    @State private var code = ""
    @FocusState private var editing: Bool
    private static let codeLength = Int(PairingOfferKt.PHONE_PAIRING_CODE_LENGTH)

    var body: some View {
        Form {
            Section {
                TextField("Computer address", text: $address)
                    .keyboardType(.URL)
                    .accessibilityIdentifier("computer-address")
                SecureField("Pairing code", text: $code)
                    .keyboardType(.numberPad)
                    .accessibilityIdentifier("computer-code")
                    .onChange(of: code) { value in
                        code = String(value.filter { ("0"..."9").contains($0) }.prefix(Self.codeLength))
                    }
            } header: {
                Text("Use the address and pairing code shown on your computer.")
            }
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .focused($editing)
            .disabled(operation.busy)
            Section {
                Button("Pair") {
                    editing = false
                    pair(address.trimmingCharacters(in: .whitespacesAndNewlines), code)
                }
                .disabled(!valid || operation.busy)
                ComputerFeedback(operation: operation)
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .scrollContentBackground(.hidden)
        .background(ConstructTheme.color(.background))
        .onChange(of: operation.needsFreshCode) { if $0 { code = "" } }
    }

    private var valid: Bool {
        !address.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && code.count == Self.codeLength
    }
}
