#if STAYA_PROBE
import Foundation

/// Метрика для сборщика этапа 1 (tools/probe-server, docs/server.md).
///
/// Имена полей и значения перечислений должны совпадать со схемой сервера байт в
/// байт — он отвергает всё лишнее. Координат здесь нет и быть не должно: в
/// метрику попадают только точность замера и корзина скорости.
struct ProbeRecord: Codable, Equatable, Sendable {
    enum Platform: String, Codable, Sendable { case ios, android }

    enum Trigger: String, Codable, Sendable {
        case significantChange = "significant_change"
        case visit
        case continuous
        case timer
        case motion
        case foreground
        case boot
        case continuousStart = "continuous_start"
        case continuousStop = "continuous_stop"
    }

    enum AppState: String, Codable, Sendable { case foreground, background, relaunched }

    enum Speed: String, Codable, Sendable {
        case unknown, still, walking, driving

        /// Метры в секунду из CLLocation.speed; отрицательное — неизвестно.
        init(metersPerSecond v: Double) {
            switch v {
            case ..<0: self = .unknown
            case ..<0.5: self = .still
            case ..<3.0: self = .walking
            default: self = .driving
            }
        }
    }

    enum Auth: String, Codable, Sendable {
        case always
        case whenInUse = "when_in_use"
        case denied
        case restricted
        case notDetermined = "not_determined"
    }

    enum BgRefresh: String, Codable, Sendable { case available, denied, restricted }

    var device: String
    var platform: Platform = .ios
    var strategy: String
    /// Unix-время события, секунды (целое).
    var eventTs: Int64
    var trigger: Trigger
    var appState: AppState
    /// Точность замера в метрах, округлённая; `nil`, если замера нет или точность < 0.
    var accuracyM: UInt32?
    var speed: Speed
    var batteryPct: UInt8
    var charging: Bool
    var lowPower: Bool
    var prevSendMs: UInt32?
    var prevSendFailures: UInt32
    var auth: Auth
    var precise: Bool
    var bgRefresh: BgRefresh?
    var eventsSinceLast: UInt32

    enum CodingKeys: String, CodingKey {
        case device, platform, strategy, trigger, speed, charging, auth, precise
        case eventTs = "event_ts"
        case appState = "app_state"
        case accuracyM = "accuracy_m"
        case batteryPct = "battery_pct"
        case lowPower = "low_power"
        case prevSendMs = "prev_send_ms"
        case prevSendFailures = "prev_send_failures"
        case bgRefresh = "bg_refresh"
        case eventsSinceLast = "events_since_last"
    }

    /// Сервер ждёт `null`, а не отсутствие поля, у необязательных значений.
    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(device, forKey: .device)
        try c.encode(platform, forKey: .platform)
        try c.encode(strategy, forKey: .strategy)
        try c.encode(eventTs, forKey: .eventTs)
        try c.encode(trigger, forKey: .trigger)
        try c.encode(appState, forKey: .appState)
        try c.encode(accuracyM, forKey: .accuracyM)
        try c.encode(speed, forKey: .speed)
        try c.encode(batteryPct, forKey: .batteryPct)
        try c.encode(charging, forKey: .charging)
        try c.encode(lowPower, forKey: .lowPower)
        try c.encode(prevSendMs, forKey: .prevSendMs)
        try c.encode(prevSendFailures, forKey: .prevSendFailures)
        try c.encode(auth, forKey: .auth)
        try c.encode(precise, forKey: .precise)
        try c.encode(bgRefresh, forKey: .bgRefresh)
        try c.encode(eventsSinceLast, forKey: .eventsSinceLast)
    }

    /// Точность замера: отрицательная у CoreLocation значит «нет данных».
    static func accuracy(_ meters: Double) -> UInt32? {
        guard meters >= 0, meters.isFinite else { return nil }
        return UInt32(min(meters.rounded(), Double(UInt32.max)))
    }
}
#endif
