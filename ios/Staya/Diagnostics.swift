import Foundation
import Security

/// Проверки окружения для задачи 1.1a: работает ли Keychain в сборке,
/// переподписанной AltStore/SideStore своим Apple ID.
enum Diagnostics {
    /// Пишет, читает и удаляет тестовый секрет с тем же уровнем доступа,
    /// что будет у ключа базы (docs/protocol.md §3).
    static func keychainRoundTrip() -> Bool {
        let account = "staya.diagnostics.\(UUID().uuidString)"
        let secret = Data((0..<32).map { _ in UInt8.random(in: 0...255) })
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "staya.diagnostics",
            kSecAttrAccount as String: account,
        ]
        defer { SecItemDelete(base as CFDictionary) }

        var add = base
        add[kSecValueData as String] = secret
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else { return false }

        var query = base
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return false }
        return (result as? Data) == secret
    }
}
