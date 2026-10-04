#if DEBUG
import Foundation
import StayaCore

/// Проверка на симуляторе в CI (scripts/test-ios-simulator.sh): настоящий Keychain и
/// ядро. Запуск с аргументом `-staya-selftest`; печатает итог и завершает процесс.
enum SelfTest {
    static func runIfRequested() {
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "-staya-devexchange"), i + 1 < args.count, let url = URL(string: args[i + 1]) {
            // Сеть асинхронная: запуск приложения продолжается, итог — exit() из задачи.
            Task.detached { await devExchange(server: url) }
            return
        }
        guard args.contains("-staya-selftest") else { return }
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

    // MARK: - Обмен с dev-peer (задача 2.10)

    // Должны совпадать с аргументами dev-peer в scripts/test-ios-simulator.sh.
    private static let mine = (lat: Int32(599_386_000), lon: Int32(303_141_000))
    private static let peer = (lat: Int32(557_558_000), lon: Int32(376_173_000))

    /// Временная база, приглашение с `/dev/invite`, обмен позициями с тестовым собеседником.
    private static func devExchange(server: URL) async {
        do {
            try await exchange(server: server)
            print("DEVEXCHANGE OK")
            exit(0)
        } catch {
            print("DEVEXCHANGE FAIL: \(error)")
            exit(1)
        }
    }

    private static func exchange(server: URL) async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let core = try StayaCore.open(dbPath: dir.appendingPathComponent("staya.db").path, dbKey: try DbKey.random())
        let http = StayaHTTP(base: server, token: try core.identity().accountId)
        let sync = CoreSync(core: core, http: http)
        try await sync.publishKeys()

        let deadline = Date().addingTimeInterval(300)
        var invite: String?
        while invite == nil {
            invite = try? await http.getPublic("/dev/invite")
            if invite == nil {
                try expect(Date() < deadline, "no invite from peer")
                try await Task.sleep(for: .milliseconds(500))
            }
        }
        try await sync.accept(invite!)

        var gotAt: Date?
        while gotAt.map({ Date().timeIntervalSince($0) < 20 }) ?? true {
            try expect(Date() < deadline, "no location from peer")
            for event in try await sync.sync() {
                if case .locationUpdated(_, let location) = event, gotAt == nil,
                   location.latE7 == peer.lat, location.lonE7 == peer.lon {
                    gotAt = Date()
                }
            }
            if try core.listFriends().contains(where: { $0.active }) {
                let now = Int64(Date().timeIntervalSince1970)
                try await sync.share(Location(latE7: mine.lat, lonE7: mine.lon, accuracyM: 10, timestamp: now))
            }
            try await Task.sleep(for: .seconds(1))
        }
    }
}
#endif
