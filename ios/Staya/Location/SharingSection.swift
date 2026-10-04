import CoreLocation
import StayaCore
import SwiftUI
import UIKit

/// «Делиться позицией» (задача 4.5). Разрешения — только по нажатию: сначала «при
/// использовании», затем «всегда» (без него позиция не уходит в фоне).
struct SharingSection: View {
    let core: StayaCore
    @State private var engine = LocationEngine.shared

    var body: some View {
        Section {
            Toggle(isOn: Binding(
                get: { engine.enabled },
                set: { $0 ? engine.enable(core: core) : engine.disable(core: core) }
            )) {
                VStack(alignment: .leading) {
                    Text("Делиться позицией")
                    Text(engine.enabled ? "Друзья видят, где ты" : "Выключено — друзья видят «скрыл(а) позицию»")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            if engine.enabled {
                switch engine.authorization {
                case .authorizedAlways:
                    EmptyView()
                case .authorizedWhenInUse:
                    Text("Чтобы позиция уходила, когда приложение закрыто, выбери «Всегда».")
                        .font(.footnote)
                    Button("Разрешить «Всегда»") { engine.requestAuthorization() }
                    settingsButton
                case .denied, .restricted:
                    Text("Геолокация для Staya выключена — друзья не увидят позицию.")
                        .font(.footnote)
                        .foregroundStyle(.red)
                    settingsButton
                default:
                    EmptyView()
                }
            }
        }
    }

    private var settingsButton: some View {
        Button("Открыть настройки") {
            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
        }
    }
}
