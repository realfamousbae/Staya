import Foundation

// Правила docs/protocol.md §3.1 (DbKey.obtain) на подделках хранилища и базы.

final class FakeStore: KeyBackend {
    var stored: Data?
    var failRead: SecretRead?
    var failAdd = false
    var adds = 0

    init(_ stored: Data? = nil) { self.stored = stored }

    func read() -> SecretRead {
        if let failRead { return failRead }
        return stored.map { .found($0) } ?? .notFound
    }

    func addIfAbsent(_ value: Data) throws {
        adds += 1
        if failAdd { throw DbKeyError.keychain(-1) }
        if stored == nil { stored = value }
    }
}

final class FakeDb {
    var exists: Bool
    var deletes = 0
    init(exists: Bool) { self.exists = exists }
    func delete() { deletes += 1; exists = false }
}

let fresh = Data(repeating: 1, count: 32)

func obtain(_ store: FakeStore, _ db: FakeDb) -> DbKey.Result {
    DbKey.obtain(store: store, dbExists: { db.exists }, deleteDb: db.delete, newKey: { fresh })
}

func check(_ ok: Bool, _ name: String) {
    guard ok else { print("FAIL: \(name)"); exit(1) }
    print("ok: \(name)")
}

do {
    let store = FakeStore(), db = FakeDb(exists: false)
    check(obtain(store, db) == .ready(fresh) && store.stored == fresh && db.deletes == 0, "first launch creates key")
}
do {
    let old = Data(repeating: 9, count: 32)
    let store = FakeStore(old), db = FakeDb(exists: true)
    check(obtain(store, db) == .ready(old) && store.adds == 0 && db.deletes == 0, "existing key is reused")
}
do {
    let store = FakeStore(), db = FakeDb(exists: true)
    store.failRead = .unavailable("errSecInteractionNotAllowed")
    if case .unavailable = obtain(store, db) {} else { check(false, "transient error") }
    check(store.adds == 0 && store.stored == nil && db.exists, "transient error never creates key or touches db")
}
do {
    let store = FakeStore(), db = FakeDb(exists: true)
    store.failRead = .broken("bad")
    if case .broken = obtain(store, db) {} else { check(false, "broken key") }
    check(store.adds == 0 && db.exists, "broken key is reported, not replaced")
}
do {
    let store = FakeStore(), db = FakeDb(exists: true)
    check(obtain(store, db) == .ready(fresh) && db.deletes == 1 && !db.exists, "lost key discards unreadable db")
}
do {
    let store = FakeStore(), db = FakeDb(exists: false)
    store.failAdd = true
    if case .unavailable = obtain(store, db) {} else { check(false, "failed save") }
    check(store.stored == nil, "failed save does not open")
}
do {
    if case .broken = obtain(FakeStore(Data(count: 16)), FakeDb(exists: true)) {
        check(true, "wrong size is broken")
    } else {
        check(false, "wrong size is broken")
    }
}
check((try? DbKey.random())?.count == 32, "random key is 32 bytes")
print("DbKey: all passed")
