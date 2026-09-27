import StayaCore
import SwiftUI

struct ContentView: View {
    var body: some View {
        VStack(spacing: 8) {
            Text("Staya")
                .font(.largeTitle.bold())
            Text("Ядро \(coreVersion())")
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}

#Preview {
    ContentView()
}
