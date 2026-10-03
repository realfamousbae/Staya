#if STAYA_PROBE
import CoreLocation
import StayaCore
import SwiftUI

/// Отладочный экран прототипа этапа 1: стратегия, токен сборщика, статусы, счётчики.
struct ProbeDebugView: View {
    @State private var engine = LocationEngine.shared
    @State private var tokenInput = ""
    @State private var tokenSaved = LocationEngine.shared.hasToken
    @State private var keychainOK: Bool?

    var body: some View {
        NavigationStack {
            Form {
                Section("Замер") {
                    Picker("Стратегия", selection: Binding(get: { engine.strategy }, set: { engine.select($0) })) {
                        ForEach(LocationEngine.Strategy.allCases) { Text($0.title).tag($0) }
                    }
                    Toggle("Запущено", isOn: Binding(get: { engine.running }, set: { engine.setRunning($0) }))
                    row("Непрерывные обновления", engine.continuousOn ? "вкл" : "выкл")
                }

                Section("Разрешения") {
                    row("Геолокация", authText)
                    row("Точность", engine.precise ? "точная" : "примерная")
                    row("Фоновое обновление", engine.backgroundRefresh)
                    row("Энергосбережение", ProcessInfo.processInfo.isLowPowerModeEnabled ? "вкл" : "выкл")
                    row("Датчик движения (S4)", engine.motionAuthorization)
                    if engine.authorization != .authorizedAlways {
                        Button("Разрешить геолокацию «Всегда»") { engine.requestAuthorization() }
                    }
                }

                Section("Сборщик метрик") {
                    if tokenSaved {
                        row("Токен", "сохранён в Keychain")
                    }
                    SecureField("Токен с сервера", text: $tokenInput)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button("Сохранить токен") {
                        tokenSaved = engine.saveToken(tokenInput)
                        tokenInput = ""
                    }
                    .disabled(tokenInput.trimmingCharacters(in: .whitespaces).count < 32)
                    Button("Тестовая отправка") { engine.sendTest() }
                        .disabled(!tokenSaved)
                }

                Section("Счётчики") {
                    row("Событий", "\(engine.eventsTotal)")
                    row("Отправлено", "\(engine.sentTotal)")
                    row("Сбоев", "\(engine.failedTotal)")
                    row("В очереди", "\(engine.queued)")
                    row("Последний ответ", engine.lastStatus)
                    row("Последнее событие", engine.lastEventAt.map { $0.formatted(date: .omitted, time: .standard) } ?? "—")
                    Button("Сбросить счётчики", role: .destructive) { engine.resetCounters() }
                }

                Section("Окружение") {
                    row("Ядро", coreVersion())
                    row("Keychain", keychainOK.map { $0 ? "OK" : "ошибка" } ?? "…")
                }
            }
            .navigationTitle("Staya · замер")
            .task { keychainOK = Diagnostics.keychainRoundTrip() }
        }
    }

    private var authText: String {
        switch engine.authorization {
        case .authorizedAlways: "всегда"
        case .authorizedWhenInUse: "при использовании"
        case .denied: "запрещена"
        case .restricted: "ограничена"
        case .notDetermined: "не спрашивали"
        @unknown default: "?"
        }
    }

    private func row(_ title: String, _ value: String) -> some View {
        LabeledContent(title, value: value)
    }
}
#endif
