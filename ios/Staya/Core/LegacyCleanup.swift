import Foundation
import Security

/// Остатки прототипа замеров этапа 1 (удалён в 4.5): токен сборщика метрик в
/// Keychain (переживает и переустановку), очередь и настройки. Обновление их не
/// стирает; удаляем при каждом запуске — это дёшево, а записей уже нет.
enum LegacyCleanup {
    static func run() {
        SecItemDelete([
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "staya.probe",
        ] as CFDictionary)
        if let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            try? FileManager.default.removeItem(at: dir.appendingPathComponent("probe-queue.json"))
        }
        let defaults = UserDefaults.standard
        for key in defaults.dictionaryRepresentation().keys where key.hasPrefix("probe.") {
            defaults.removeObject(forKey: key)
        }
    }
}
