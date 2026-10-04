import StayaCore
import SwiftUI

struct ContentView: View {
    @State private var core = AppCore.shared

    var body: some View {
        #if STAYA_PROBE
        ProbeDebugView()
        #else
        VStack(spacing: 8) {
            Text("Staya").font(.largeTitle.bold())
            Text("Ядро \(coreVersion())").foregroundStyle(.secondary)
            Text("Аккаунт \(core.state.summary)").foregroundStyle(.secondary)
            if core.state.isBroken {
                Button("Сбросить локальные данные", role: .destructive) { core.reset() }
            }
        }
        .task { core.open() }
        #endif
    }
}

#Preview {
    ContentView()
}
