import Foundation
import StayaCore

/// Экспоненциальная задержка с разбросом (как `Backoff` на Android); после 429 —
/// не меньше 30 с: лимиты сервера общие для всех за одним NAT.
struct Backoff {
    var baseMs: Double = 1_000
    var maxMs: Double = 300_000
    private var attempt = 0

    mutating func next(rateLimited: Bool = false) -> Duration {
        let exp = min(baseMs * pow(2, Double(min(attempt, 20))), maxMs)
        attempt += 1
        let delay = exp / 2 + Double.random(in: 0...(exp / 2))
        return .milliseconds(Int(rateLimited ? max(delay, 30_000) : delay))
    }

    mutating func reset() { attempt = 0 }
}

/// Живая доставка, пока приложение на экране (protocol §8.2.6): WebSocket, после
/// подключения — один полный забор ящика, затем события по одному; обрыв — новое
/// подключение с `Backoff`. Ключ сервера отвергнут — без переподключений (решает
/// пользователь, 4.2). В фоне не используется.
actor LiveConnection {
    private let client: StayaClient
    private let sync: CoreSync
    private let queue: SerialQueue
    private let onEvents: @Sendable ([CoreEvent]) -> Void
    private let onError: @Sendable (Error) -> Void
    private var loop: Task<Void, Never>?

    init(
        client: StayaClient,
        sync: CoreSync,
        queue: SerialQueue = SerialQueue(),
        onEvents: @escaping @Sendable ([CoreEvent]) -> Void,
        onError: @escaping @Sendable (Error) -> Void = { _ in }
    ) {
        self.client = client
        self.sync = sync
        self.queue = queue
        self.onEvents = onEvents
        self.onError = onError
    }

    func start() {
        guard loop == nil else { return }
        loop = Task { await run() }
    }

    func stop() {
        loop?.cancel()
        loop = nil
    }

    private func run() async {
        var backoff = Backoff()
        while !Task.isCancelled {
            var rateLimited = false
            do {
                let socket = try await client.webSocket()
                socket.resume()
                defer { socket.cancel(with: .normalClosure, reason: nil) }
                do {
                    let sync = self.sync
                    onEvents(try await queue.run { try await sync.sync() })
                    backoff.reset()
                    while !Task.isCancelled {
                        if case .string(let text) = try await socket.receive() {
                            onEvents(try await queue.run { try await sync.handleWsEvent(text) })
                        }
                    }
                } catch {
                    if client.wasRejected(socket) { throw StayaNetError.serverKeyRejected }
                    throw error
                }
            } catch StayaNetError.serverKeyRejected {
                onError(StayaNetError.serverKeyRejected)
                return
            } catch {
                onError(error)
                rateLimited = (error as? StayaNetError) == .rateLimited
            }
            try? await Task.sleep(for: backoff.next(rateLimited: rateLimited))
        }
    }
}
