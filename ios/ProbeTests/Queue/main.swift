// Тест очереди метрик (ProbeQueue): запись, пришедшая во время отправки, не теряется.
import Foundation
let url = FileManager.default.temporaryDirectory.appendingPathComponent("q-\(UUID().uuidString).json")
func rec(_ s: String) -> ProbeRecord {
    ProbeRecord(device: "d", strategy: s, eventTs: 1, trigger: .visit, appState: .background, accuracyM: nil,
                speed: .unknown, batteryPct: 1, charging: false, lowPower: false, prevSendMs: nil,
                prevSendFailures: 0, auth: .always, precise: true, bgRefresh: nil, eventsSinceLast: 1)
}
var q = ProbeQueue(url: url)
q.append(rec("first"))
let inFlight = q.peek(5)                 // отправка началась
q.append(rec("arrived-during-send"))     // новая запись, пока ждём сеть
for item in inFlight { q.remove(id: item.id) }  // отправка закончилась
precondition(q.items.map(\.record.strategy) == ["arrived-during-send"], "lost a record: \(q.items)")
let reloaded = ProbeQueue(url: url)       // перезапуск приложения
precondition(reloaded.items.map(\.record.strategy) == ["arrived-during-send"])
var small = ProbeQueue(url: url.appendingPathExtension("2"), limit: 3)
for i in 0..<5 { small.append(rec("r\(i)")) }
precondition(small.items.map(\.record.strategy) == ["r2", "r3", "r4"])
// Обёртка с id не попадает в тело запроса: кодируется только ProbeRecord.
let body = String(data: try! JSONEncoder().encode(reloaded.items[0].record), encoding: .utf8)!
precondition(!body.contains("\"id\""))
print("queue tests ok")
