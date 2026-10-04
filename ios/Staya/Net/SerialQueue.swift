import Foundation

/// Последовательное выполнение асинхронных операций: отправки ядра (`pending_sends`
/// → POST → `complete_send`) не должны идти параллельно из UI и WebSocket. Актор
/// здесь не подходит — он пускает следующий вызов на каждом `await`.
final class SerialQueue: @unchecked Sendable {
    private let lock = NSLock()
    private var tail: Task<Void, Never>?

    func run<T: Sendable>(_ operation: @escaping @Sendable () async throws -> T) async throws -> T {
        try await enqueue(operation).value
    }

    /// Синхронно ставит операцию в конец очереди (под замком, без `await`).
    private func enqueue<T: Sendable>(_ operation: @escaping @Sendable () async throws -> T) -> Task<T, Error> {
        lock.withLock {
            let previous = tail
            let task = Task {
                await previous?.value
                return try await operation()
            }
            tail = Task { _ = try? await task.value }
            return task
        }
    }
}
