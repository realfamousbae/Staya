import Foundation

/// Замер с устройства без CoreLocation (проверяется на Mac: ios/CoreTests/Location).
/// Никогда не печатать `CLLocation` — в его описании координаты; у `Fix` они скрыты.
struct Fix: Sendable, CustomStringConvertible {
    let latitude: Double
    let longitude: Double
    /// Отрицательная — точность неизвестна (так её отдаёт CoreLocation).
    let accuracyM: Double
    let timestamp: Date

    var description: String { "Fix(<redacted>, t=\(Int64(timestamp.timeIntervalSince1970)))" }
}

/// Правила отправки позиции (задача 4.5).
enum LocationPolicy {
    /// Не чаще: в машине при шаге 100 м точки идут каждые несколько секунд.
    static let minSendInterval: TimeInterval = 60

    /// Позиция для ядра (градусы × 10⁷) или `nil`, если замер негоден. Время — время замера.
    static func toCore(_ fix: Fix) -> (latE7: Int32, lonE7: Int32, accuracyM: UInt16, timestamp: Int64)? {
        guard fix.accuracyM.isFinite, fix.accuracyM >= 0,
              fix.latitude.isFinite, fix.longitude.isFinite,
              (-90...90).contains(fix.latitude), (-180...180).contains(fix.longitude)
        else { return nil }
        return (
            Int32((fix.latitude * 1e7).rounded()),
            Int32((fix.longitude * 1e7).rounded()),
            UInt16(min(fix.accuracyM.rounded(.up), Double(UInt16.max))),
            Int64(fix.timestamp.timeIntervalSince1970.rounded(.down))
        )
    }

    static func shouldSend(now: Date, lastSent: Date?) -> Bool {
        guard let lastSent else { return true }
        return now.timeIntervalSince(lastSent) >= minSendInterval || now < lastSent
    }
}

/// Ограничение частоты с придержанной точкой: отброшенный по частоте замер
/// запоминается и уходит, когда обновления остановились (машина припарковалась) —
/// иначе на карте осталась бы точка до минуты езды назад.
struct SendThrottle {
    private(set) var lastSent: Date?
    private var held: Fix?

    /// Замер, который надо отправить сейчас, или `nil` (придержан).
    mutating func offer(_ fix: Fix, now: Date) -> Fix? {
        guard LocationPolicy.shouldSend(now: now, lastSent: lastSent) else {
            held = fix
            return nil
        }
        lastSent = now
        held = nil
        return fix
    }

    /// Придержанный замер — без ограничения частоты.
    mutating func takeHeld(now: Date) -> Fix? {
        guard let fix = held else { return nil }
        held = nil
        lastSent = now
        return fix
    }

    mutating func reset() {
        lastSent = nil
        held = nil
    }
}
