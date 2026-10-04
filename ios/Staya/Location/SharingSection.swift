import CoreLocation
import StayaCore
import SwiftUI
import UIKit

/// «Делиться позицией» (задача 4.5). Разрешения — только по нажатию: сначала «при
/// использовании», затем «всегда» (без него позиция не уходит в фоне).
struct SharingSection: View {
    let core: StayaCore
    @State private var engine = LocationEngine.shared
    @State private var frozen = false
    @State private var freezeError = false

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
                Toggle(isOn: Binding(
                    get: { frozen },
                    set: { on in
                        Task {
                            let ok = await engine.setFrozen(on, core: core)
                            freezeError = on && !ok
                            if ok { frozen = on }
                        }
                    }
                )) {
                    VStack(alignment: .leading) {
                        Text("Заморозить позицию здесь")
                        Text(freezeError
                             ? "Пока нет ни одного замера — подожди, пока позиция определится."
                             : frozen ? "Друзья видят эту точку, куда бы ты ни пошёл(а)" : "Друзья увидят последнюю точку, пока не снимешь")
                            .font(.footnote)
                            .foregroundStyle(freezeError ? .red : .secondary)
                    }
                }
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
        .task { frozen = engine.isFrozen(core: core) }
    }

    private var settingsButton: some View {
        Button("Открыть настройки") {
            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
        }
    }
}
