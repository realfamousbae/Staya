#if DEBUG
import Foundation
import StayaCore

/// Проверка на симуляторе в CI (scripts/test-ios-simulator.sh): настоящий Keychain и
/// ядро. Запуск с аргументом `-staya-selftest`; печатает итог и завершает процесс.
enum SelfTest {
    static func runIfRequested() {
        guard CommandLine.arguments.contains("-staya-selftest") else { return }
        do {
            try run()
            print("SELFTEST OK")
            exit(0)
        } catch {
            print("SELFTEST FAIL: \(error)")
            exit(1)
        }
    }

    private struct Failure: Error, CustomStringConvertible {
        let description: String
    }

    private static func expect(_ ok: Bool, _ what: String) throws {
        guard ok else { throw Failure(description: what) }
    }

    private static func run() throws {
        let store = KeychainKeyStore(service: "staya.selftest", account: UUID().uuidString)
        defer { store.delete() }
        try expect(store.read() == .notFound, "empty keychain: \(store.read())")
        let first = Data(repeating: 3, count: 32)
        try store.addIfAbsent(first)
        try expect(store.read() == .found(first), "round trip: \(store.read())")
        try store.addIfAbsent(Data(repeating: 4, count: 32))
        try expect(store.read() == .found(first), "add-only")
        store.delete()

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let db = dir.appendingPathComponent("staya.db")
        func key() throws -> Data {
            let r = DbKey.obtain(
                store: store,
                dbExists: { FileManager.default.fileExists(atPath: db.path) },
                deleteDb: { try? FileManager.default.removeItem(at: db) }
            )
            guard case .ready(let key) = r else { throw Failure(description: "obtain: \(r)") }
            return key
        }
        let id = try StayaCore.open(dbPath: db.path, dbKey: try key()).identity().accountId
        let again = try StayaCore.open(dbPath: db.path, dbKey: try key()).identity().accountId
        try expect(id == again, "reopen with stored key")
    }
}
#endif
