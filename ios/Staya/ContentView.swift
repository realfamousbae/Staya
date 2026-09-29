import StayaCore
import SwiftUI

struct ContentView: View {
    @State private var keychainOK: Bool?

    var body: some View {
        VStack(spacing: 8) {
            Text("Staya")
                .font(.largeTitle.bold())
            Text("Ядро \(coreVersion())")
                .foregroundStyle(.secondary)
            Text(keychainStatus)
                .font(.footnote.monospaced())
                .foregroundStyle(keychainOK == false ? .red : .secondary)
        }
        .padding()
        .task { keychainOK = Diagnostics.keychainRoundTrip() }
    }

    private var keychainStatus: String {
        switch keychainOK {
        case .none: "Keychain: проверка…"
        case .some(true): "Keychain: OK"
        case .some(false): "Keychain: ошибка"
        }
    }
}

#Preview {
    ContentView()
}
