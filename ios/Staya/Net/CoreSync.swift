import Foundation
import StayaCore

/// Сеть вокруг ядра по правилам protocol §8.2: ядро готовит и разбирает JSON,
/// здесь — только запросы.
struct CoreSync: Sendable {
    let core: StayaCore
    let http: StayaHTTP

    private var now: Int64 { Int64(Date().timeIntervalSince1970) }

    /// Пополняет одноразовые ключи на сервере (§4.3).
    func publishKeys() async throws {
        struct Count: Decodable { let one_time_keys: UInt32 }
        let count = try JSONDecoder().decode(Count.self, from: Data(try await http.get("/v1/keys/count").utf8))
        guard let json = try core.keysToPublish(serverOtkCount: count.one_time_keys, now: now) else { return }
        _ = try await http.put("/v1/keys", json)
        try core.markKeysPublished()
    }

    func accept(_ uri: String) async throws {
        // Новый аккаунт берёт сервер из приглашения (protocol §5.3).
        _ = try core.setServerFromLink(uri: uri)
        let info = try core.parseInvite(uri: uri)
        let claim = String(decoding: try JSONEncoder().encode(["account_id": info.accountId]), as: UTF8.self)
        let claimed = try await http.post("/v1/keys/claim", claim)
        try core.acceptInvite(uri: uri, claimResponseJson: claimed, now: now)
        try await flush()
    }

    /// Удаления слотов, затем один POST со всеми конвертами, затем `completeSend`.
    func flush() async throws {
        let batch = try core.pendingSends()
        for friend in batch.deleteSlots {
            _ = try await http.delete("/v1/slots/\(friend)")
            try core.markSlotDeleted(friend: friend)
        }
        guard let json = batch.requestJson else { return }
        try core.completeSend(ids: batch.ids, responseJson: try await http.post("/v1/envelopes", json))
    }

    /// Забирает ящик, обрабатывает, подтверждает и отправляет ответы.
    func sync() async throws -> [CoreEvent] {
        let processed = try core.processMailbox(mailboxJson: try await http.get("/v1/mailbox"), now: now)
        if let ack = processed.ackJson { _ = try await http.post("/v1/mailbox/ack", ack) }
        try await flush()
        return processed.events
    }

    func share(_ location: Location) async throws {
        try core.prepareLocationUpdate(location: location, now: now)
        try await flush()
    }
}
