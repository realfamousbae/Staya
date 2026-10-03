#if STAYA_PROBE
import Foundation

/// Очередь неотправленных метрик: в памяти и на диске после каждого изменения.
///
/// Отправленное удаляется по идентификатору, а не перезаписью «старой копии»:
/// записи, добавленные во время отправки, не теряются. Идентификатор живёт
/// только в обёртке — в `ProbeRecord` его нет (сервер отвергает лишние поля).
struct ProbeQueue {
    struct Item: Codable, Equatable, Sendable {
        let id: UUID
        let record: ProbeRecord
    }

    private(set) var items: [Item]
    private let url: URL
    private let limit: Int

    init(url: URL, limit: Int = 500) {
        self.url = url
        self.limit = limit
        let data = try? Data(contentsOf: url)
        items = data.flatMap { try? JSONDecoder().decode([Item].self, from: $0) } ?? []
    }

    var count: Int { items.count }

    /// Первые `n` записей для отправки — снимок, очередь при этом не меняется.
    func peek(_ n: Int) -> [Item] { Array(items.prefix(n)) }

    mutating func append(_ record: ProbeRecord) {
        items.append(Item(id: UUID(), record: record))
        // При переполнении теряем самые старые — свежие важнее для замера.
        if items.count > limit { items.removeFirst(items.count - limit) }
        persist()
    }

    mutating func remove(id: UUID) {
        items.removeAll { $0.id == id }
        persist()
    }

    private func persist() {
        guard let data = try? JSONEncoder().encode(items) else { return }
        #if os(iOS)
        // Доступен после первой разблокировки — пишем и при заблокированном экране.
        try? data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        #else
        try? data.write(to: url, options: .atomic)
        #endif
    }
}
#endif
