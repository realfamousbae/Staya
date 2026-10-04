import Foundation
import Observation
import StayaCore

/// Единственный на процесс экземпляр ядра (база открывается эксклюзивно). Открывается
/// лениво с экрана, а не в AppDelegate: фоновый запуск по геолокации может прийти
/// до первой разблокировки, когда ключа в Keychain ещё не прочитать.
@MainActor
@Observable
final class AppCore {
    enum State {
        case closed
        case opening
        case open(StayaCore, accountId: String)
        /// Временная ошибка — откроется при следующей попытке.
        case unavailable(String)
        /// Ключ или база испорчены — только явный сброс.
        case broken(String)
    }

    static let shared = AppCore()

    private(set) var state: State = .closed

    nonisolated private static let keyStore = KeychainKeyStore(service: "staya", account: "db-key")

    func open() {
        switch state {
        case .open, .opening: return
        default: break
        }
        state = .opening
        Task.detached(priority: .userInitiated) {
            let next = Self.openCore()
            await MainActor.run { self.state = next }
        }
    }

    /// Удаляет базу и ключ и создаёт аккаунт заново. Друзей придётся добавить снова.
    func reset() {
        if case .opening = state { return }
        state = .opening
        Task.detached(priority: .userInitiated) {
            Self.deleteDb()
            Self.keyStore.delete()
            let next = Self.openCore()
            await MainActor.run { self.state = next }
        }
    }

    nonisolated private static func openCore() -> State {
        let db: URL
        do {
            db = try dbURL()
        } catch {
            return .unavailable(String(describing: error))
        }
        let result = DbKey.obtain(
            store: keyStore,
            dbExists: { FileManager.default.fileExists(atPath: db.path) },
            deleteDb: deleteDb
        )
        switch result {
        case .unavailable(let reason): return .unavailable(reason)
        case .broken(let reason): return .broken(reason)
        case .ready(let key):
            do {
                let core = try StayaCore.open(dbPath: db.path, dbKey: key)
                return .open(core, accountId: try core.identity().accountId)
            } catch CoreError.Corrupted(let message) {
                return .broken(message)
            } catch {
                return .unavailable(String(describing: error))
            }
        }
    }

    /// Application Support/staya/staya.db; каталог исключён из бэкапа (docs/threat-model.md).
    /// Защита файлов по умолчанию — до первой разблокировки: база нужна фоновым пробуждениям.
    nonisolated private static func dbURL() throws -> URL {
        var dir = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        ).appendingPathComponent("staya", isDirectory: true)
        try FileManager.default.createDirectory(
            at: dir, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
        )
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try dir.setResourceValues(values)
        return dir.appendingPathComponent("staya.db")
    }

    nonisolated private static func deleteDb() {
        guard let db = try? dbURL() else { return }
        try? FileManager.default.removeItem(at: URL(fileURLWithPath: db.path + "-journal"))
        try? FileManager.default.removeItem(at: db)
    }
}

extension AppCore.State {
    /// Короткая строка состояния для отладочных экранов (без секретов).
    var summary: String {
        switch self {
        case .closed, .opening: "открывается…"
        case .open(_, let accountId): String(accountId.prefix(8)) + "…"
        case .unavailable(let reason): "недоступно: \(reason)"
        case .broken(let reason): "база не читается: \(reason)"
        }
    }

    var isBroken: Bool {
        if case .broken = self { true } else { false }
    }
}
