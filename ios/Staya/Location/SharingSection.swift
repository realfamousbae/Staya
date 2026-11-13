import CoreLocation
import StayaCore
import SwiftUI
import UIKit

/// «Делиться позицией» (задача 4.5) на экране настроек (5.2). Разрешения — только по
/// нажатию: сначала «при использовании», затем «всегда» (без него позиция не уходит в фоне).
struct SharingSection: View {
    let core: StayaCore
    @State private var engine = LocationEngine.shared
    @State private var frozen = false
    @State private var freezeError = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionTitle("передача")
            toggleRow(
                title: "Делиться позицией",
                subtitle: engine.enabled ? "Друзья видят, где ты" : "Выключено — друзья видят «скрыл(а) позицию»",
                isOn: Binding(
                    get: { engine.enabled },
                    set: { $0 ? engine.enable(core: core) : engine.disable(core: core) }
                )
            )
            if engine.enabled {
                toggleRow(
                    title: "Заморозить позицию здесь",
                    subtitle: freezeError
                        ? "Пока нет ни одного замера — подожди, пока позиция определится."
                        : frozen ? "Друзья видят эту точку, куда бы ты ни пошёл(а)" : "Друзья увидят последнюю точку, пока не снимешь",
                    subtitleColor: freezeError ? Ink.danger : Ink.mist,
                    isOn: Binding(
                        get: { frozen },
                        set: { on in
                            Task {
                                let ok = await engine.setFrozen(on, core: core)
                                freezeError = on && !ok
                                if ok { frozen = on }
                            }
                        }
                    )
                )
                if engine.reducedAccuracy {
                    note("«Точная геопозиция» выключена — друзья видят тебя с точностью до нескольких километров.")
                    settingsButton
                }
                switch engine.authorization {
                case .authorizedAlways:
                    EmptyView()
                case .authorizedWhenInUse:
                    note("Чтобы позиция уходила, когда приложение закрыто, выбери «Всегда».")
                    SoftButton(text: "Разрешить «Всегда»") { engine.requestAuthorization() }
                    settingsButton
                case .denied, .restricted:
                    Text("Геолокация для Staya выключена — друзья не увидят позицию.")
                        .font(.footnote).foregroundStyle(Ink.danger)
                    settingsButton
                default:
                    EmptyView()
                }
            }
        }
        .task { frozen = engine.isFrozen(core: core) }
    }

    private func toggleRow(title: String, subtitle: String, subtitleColor: Color = Ink.mist, isOn: Binding<Bool>) -> some View {
        Toggle(isOn: isOn) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(Ink.heading).foregroundStyle(Ink.paper)
                Text(subtitle).font(.footnote).foregroundStyle(subtitleColor)
            }
        }
        .tint(Ink.mint)
        .padding(.vertical, 6)
    }

    private func note(_ text: String) -> some View {
        Text(text).font(.footnote).foregroundStyle(Ink.mist)
    }

    private var settingsButton: some View {
        SoftButton(text: "Открыть настройки") {
            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
        }
    }
}
