import Foundation
import Security

/// Результат чтения секрета из хранилища.
enum SecretRead: Equatable {
    case found(Data)
    case notFound
    /// Хранилище временно недоступно (до первой разблокировки) — повторить позже.
    case unavailable(String)
    /// Секрет есть, но им нельзя воспользоваться.
    case broken(String)
}

/// Хранилище одного секрета; в тестах — подделка.
protocol KeyBackend {
    func read() -> SecretRead
    /// Добавляет, только если секрета ещё нет. Существующий никогда не перезаписывается.
    func addIfAbsent(_ value: Data) throws
}

/// Получение ключа локальной базы по правилам docs/protocol.md §3.1. Главное правило:
/// новый ключ создаётся только когда старого точно нет — при временной ошибке
/// хранилища новый ключ навсегда отрезал бы существующую базу.
enum DbKey {
    static let size = 32

    enum Result: Equatable {
        case ready(Data)
        case unavailable(String)
        case broken(String)
    }

    static func obtain(
        store: KeyBackend,
        dbExists: () -> Bool,
        deleteDb: () -> Void,
        newKey: () throws -> Data = DbKey.random
    ) -> Result {
        switch store.read() {
        case .found(let key): return ready(key)
        case .unavailable(let reason): return .unavailable(reason)
        case .broken(let reason): return .broken(reason)
        case .notFound: break
        }
        // Ключа точно нет: база без него нечитаема — удаляем её до того, как появится новый ключ.
        if dbExists() { deleteDb() }
        do {
            try store.addIfAbsent(try newKey())
        } catch {
            return .unavailable(String(describing: error))
        }
        // Читаем обратно: открываем базу только ключом, который действительно сохранён.
        switch store.read() {
        case .found(let key): return ready(key)
        case .notFound: return .unavailable("key was not saved")
        case .unavailable(let reason): return .unavailable(reason)
        case .broken(let reason): return .broken(reason)
        }
    }

    static func random() throws -> Data {
        var bytes = [UInt8](repeating: 0, count: size)
        let status = SecRandomCopyBytes(kSecRandomDefault, size, &bytes)
        guard status == errSecSuccess else { throw DbKeyError.random(status) }
        return Data(bytes)
    }

    private static func ready(_ key: Data) -> Result {
        key.count == size ? .ready(key) : .broken("key has \(key.count) bytes")
    }
}

enum DbKeyError: Error {
    case random(OSStatus)
    case keychain(OSStatus)
}
