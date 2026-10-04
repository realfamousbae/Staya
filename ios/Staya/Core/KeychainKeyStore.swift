import Foundation
import Security

/// Секрет в Keychain: `AfterFirstUnlockThisDeviceOnly`, без синхронизации — доступен
/// фоновым пробуждениям после первой разблокировки, не уходит в iCloud и на другое устройство.
struct KeychainKeyStore: KeyBackend {
    let service: String
    let account: String

    private var base: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    func read() -> SecretRead {
        var query = base
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data else { return .broken("keychain item has no data") }
            return .found(data)
        case errSecItemNotFound:
            return .notFound
        default:
            // errSecInteractionNotAllowed (до первой разблокировки) и прочие сбои — временные.
            return .unavailable("keychain status \(status)")
        }
    }

    func addIfAbsent(_ value: Data) throws {
        var add = base
        add[kSecValueData as String] = value
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        add[kSecAttrSynchronizable as String] = false
        let status = SecItemAdd(add as CFDictionary, nil)
        // Уже есть — оставляем существующий: его вернёт следующее чтение.
        guard status == errSecSuccess || status == errSecDuplicateItem else {
            throw DbKeyError.keychain(status)
        }
    }

    /// Только для явного сброса пользователем.
    func delete() {
        SecItemDelete(base as CFDictionary)
    }
}
