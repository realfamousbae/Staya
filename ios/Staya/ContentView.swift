import StayaCore
import SwiftUI

struct ContentView: View {
    var body: some View {
        #if STAYA_PROBE
        ProbeDebugView()
        #else
        VStack(spacing: 8) {
            Text("Staya").font(.largeTitle.bold())
            Text("Ядро \(coreVersion())").foregroundStyle(.secondary)
        }
        #endif
    }
}

#Preview {
    ContentView()
}
