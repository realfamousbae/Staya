#if STAYA_PROBE
import CoreLocation
import CoreMotion
import Foundation
import Observation
import UIKit

/// Прототип фоновой геолокации этапа 1 (docs/PLAN.md, задача 1.2).
///
/// Создаётся в `AppDelegate` при запуске, в том числе когда iOS перезапускает
/// приложение в фоне по SLC или визиту: тогда сцены нет и SwiftUI-экраны не
/// создаются. Координаты не сохраняются и не отправляются — только метрики
/// (`ProbeRecord`). Никогда не печатать `CLLocation`: в описании есть координаты.
///
/// Что считается событием: `significant_change` и `visit` — только настоящие
/// пробуждения системой; первая точка после явного запуска замера из экрана
/// помечается `foreground`, чтобы не завышать число пробуждений.
@MainActor
@Observable
final class LocationEngine: NSObject {
    enum Strategy: String, CaseIterable, Identifiable, Sendable {
        /// Только значимые изменения местоположения.
        case s1
        /// SLC + визиты.
        case s2
        /// SLC + непрерывные обновления (`distanceFilter` 100 м), без автопаузы.
        case s3
        /// SLC + визиты; непрерывные — при движении по CoreMotion, автопауза iOS в покое.
        case s4

        var id: String { rawValue }

        var title: String {
            switch self {
            case .s1: "S1 · SLC"
            case .s2: "S2 · SLC + визиты"
            case .s3: "S3 · непрерывно"
            case .s4: "S4 · адаптивно"
            }
        }
    }

    static let shared = LocationEngine()

    // Состояние для отладочного экрана.
    private(set) var strategy: Strategy
    private(set) var running: Bool
    private(set) var continuousOn = false
    private(set) var authorization: CLAuthorizationStatus = .notDetermined
    private(set) var precise = true
    private(set) var eventsTotal: Int
    private(set) var sentTotal: Int
    private(set) var failedTotal: Int
    private(set) var queued = 0
    private(set) var lastStatus: String = "—"
    private(set) var lastEventAt: Date?

    private let manager = CLLocationManager()
    private let motion = CMMotionActivityManager()
    private let defaults = UserDefaults.standard
    private var queue: ProbeQueue

    /// Стратегия, службы которой сейчас включены (`nil` — ничего не включено).
    private var appliedStrategy: Strategy?
    /// Последний известный статус разрешения: колбэк приходит и без изменений.
    private var lastAuth: CLAuthorizationStatus?
    /// Приложение запущено системой из-за события геолокации.
    private var launchedForLocation = false
    /// Следующая точка — ответ на явный запуск из экрана, а не пробуждение системой.
    private var expectStartFix = false
    private var eventsSinceLast: UInt32 = 0
    private var lastSendAt: Date = .distantPast
    private var prevSendMs: UInt32?
    private var prevSendFailures: UInt32 = 0
    private var sending = false

    /// Непрерывные обновления шлём не чаще, чтобы радио не перекрыло расход геолокации.
    private let minSendInterval: TimeInterval = 45

    private let deviceID: String

    override private init() {
        strategy = Strategy(rawValue: defaults.string(forKey: Keys.strategy) ?? "") ?? .s1
        running = defaults.bool(forKey: Keys.running)
        eventsTotal = defaults.integer(forKey: Keys.events)
        sentTotal = defaults.integer(forKey: Keys.sent)
        failedTotal = defaults.integer(forKey: Keys.failed)
        if let id = defaults.string(forKey: Keys.device) {
            deviceID = id
        } else {
            deviceID = "ios-" + UUID().uuidString.prefix(8).lowercased()
            defaults.set(deviceID, forKey: Keys.device)
        }
        queue = ProbeQueue(url: Self.queueURL)
        super.init()
        queued = queue.count
        manager.delegate = self
        authorization = manager.authorizationStatus
        precise = manager.accuracyAuthorization == .fullAccuracy
        UIDevice.current.isBatteryMonitoringEnabled = true
    }

    // MARK: Жизненный цикл

    /// Вызывается из `AppDelegate.didFinishLaunching` — до появления какого-либо экрана.
    func start(launchedForLocation: Bool) {
        self.launchedForLocation = launchedForLocation
        // После перезапуска службы нужно включить снова, но не останавливая: остановка
        // и новый запуск SLC дали бы лишнюю точку. Событие, из-за которого iOS
        // перезапустила приложение, придёт в делегат с app_state = relaunched.
        if running { apply(strategy, restoring: true) }
    }

    func requestAuthorization() {
        switch manager.authorizationStatus {
        case .notDetermined: manager.requestWhenInUseAuthorization()
        case .authorizedWhenInUse: manager.requestAlwaysAuthorization()
        default: break
        }
    }

    func select(_ new: Strategy) {
        guard new != strategy else { return }
        strategy = new
        defaults.set(new.rawValue, forKey: Keys.strategy)
        if new == .s4 { requestMotionPermission() }
        if running {
            expectStartFix = isActive
            apply(new)
        }
    }

    func setRunning(_ on: Bool) {
        running = on
        defaults.set(on, forKey: Keys.running)
        if on {
            if strategy == .s4 { requestMotionPermission() }
            expectStartFix = isActive
            apply(strategy)
        } else {
            stopAll()
        }
    }

    /// Отправка `foreground` из отладочного экрана: проверка всей цепочки одной кнопкой.
    func sendTest() {
        record(.foreground, location: nil, force: true)
    }

    func resetCounters() {
        eventsTotal = 0
        sentTotal = 0
        failedTotal = 0
        for key in [Keys.events, Keys.sent, Keys.failed] { defaults.removeObject(forKey: key) }
    }

    private var isActive: Bool { UIApplication.shared.applicationState == .active }

    // MARK: Стратегии

    /// Выключает всё, что могла включить любая стратегия: SLC и визиты сохраняются
    /// между запусками, и остаток одной стратегии испортил бы замер другой.
    private func stopAll() {
        manager.stopMonitoringSignificantLocationChanges()
        manager.stopMonitoringVisits()
        stopContinuous(record: false)
        motion.stopActivityUpdates()
        appliedStrategy = nil
    }

    /// Включает службы стратегии. Если она уже включена — ничего не делает: каждый
    /// перезапуск SLC даёт лишнюю точку и завышает число пробуждений.
    private func apply(_ s: Strategy, restoring: Bool = false) {
        if !restoring {
            if appliedStrategy == s { return }
            stopAll()
        }
        // SLC — основа всех стратегий: только он будит приложение после выгрузки.
        manager.startMonitoringSignificantLocationChanges()
        switch s {
        case .s1:
            break
        case .s2:
            manager.startMonitoringVisits()
        case .s3:
            startContinuous(record: false)
        case .s4:
            manager.startMonitoringVisits()
            checkMotion()
        }
        appliedStrategy = s
    }

    private func startContinuous(record shouldRecord: Bool) {
        guard !continuousOn else { return }
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
        manager.distanceFilter = 100
        manager.activityType = .other
        // S4: в покое iOS сама ставит обновления на паузу, а мы их выключаем
        // (иначе без новых точек выключить было бы нечем). S3 — без паузы.
        manager.pausesLocationUpdatesAutomatically = strategy == .s4
        manager.allowsBackgroundLocationUpdates = true
        manager.showsBackgroundLocationIndicator = true
        manager.startUpdatingLocation()
        continuousOn = true
        if shouldRecord { record(.continuousStart, location: nil, force: true) }
    }

    private func stopContinuous(record shouldRecord: Bool) {
        guard continuousOn else { return }
        manager.stopUpdatingLocation()
        manager.allowsBackgroundLocationUpdates = false
        continuousOn = false
        if shouldRecord { record(.continuousStop, location: nil, force: true) }
    }

    /// Адаптивная стратегия: в фоне CoreMotion не присылает обновлений, поэтому на
    /// каждом пробуждении спрашиваем недавнюю активность сами.
    private func checkMotion() {
        guard strategy == .s4, running, !continuousOn, CMMotionActivityManager.isActivityAvailable() else {
            return
        }
        let since = Date().addingTimeInterval(-5 * 60)
        motion.queryActivityStarting(from: since, to: Date(), to: .main) { activities, _ in
            MainActor.assumeIsolated {
                let moving = (activities ?? []).contains { a in
                    a.confidence != .low && (a.walking || a.running || a.cycling || a.automotive)
                }
                guard moving, self.strategy == .s4, self.running else { return }
                self.startContinuous(record: true)
            }
        }
    }

    /// Разрешение на датчик движения можно запросить только с открытым экраном:
    /// в фоне запрос молча не удаётся, и S4 никогда бы не включала обновления.
    private func requestMotionPermission() {
        guard isActive, CMMotionActivityManager.isActivityAvailable(),
              CMMotionActivityManager.authorizationStatus() == .notDetermined else { return }
        let now = Date()
        motion.queryActivityStarting(from: now.addingTimeInterval(-60), to: now, to: .main) { _, _ in }
    }

    var motionAuthorization: String {
        guard CMMotionActivityManager.isActivityAvailable() else { return "нет датчика" }
        switch CMMotionActivityManager.authorizationStatus() {
        case .authorized: return "разрешено"
        case .denied: return "запрещено"
        case .restricted: return "ограничено"
        case .notDetermined: return "не спрашивали"
        @unknown default: return "?"
        }
    }

    // MARK: Метрики

    private func record(
        _ trigger: ProbeRecord.Trigger,
        location: CLLocation?,
        accuracy: UInt32? = nil,
        force: Bool = false
    ) {
        eventsTotal += 1
        defaults.set(eventsTotal, forKey: Keys.events)
        eventsSinceLast += 1
        lastEventAt = Date()

        // Непрерывные события копим и шлём не чаще раза в minSendInterval.
        if trigger == .continuous, !force, Date().timeIntervalSince(lastSendAt) < minSendInterval {
            return
        }
        var r = makeRecord(trigger, location: location)
        if location == nil, let accuracy { r.accuracyM = accuracy }
        queue.append(r)
        queued = queue.count
        flush()
    }

    private func makeRecord(_ trigger: ProbeRecord.Trigger, location: CLLocation?) -> ProbeRecord {
        let app = UIApplication.shared
        let appState: ProbeRecord.AppState
        if launchedForLocation, app.applicationState != .active {
            appState = .relaunched
            launchedForLocation = false
        } else {
            appState = app.applicationState == .active ? .foreground : .background
        }
        let device = UIDevice.current
        let level = device.batteryLevel
        let record = ProbeRecord(
            device: deviceID,
            strategy: strategy.rawValue,
            eventTs: Int64(Date().timeIntervalSince1970),
            trigger: trigger,
            appState: appState,
            accuracyM: location.flatMap { ProbeRecord.accuracy($0.horizontalAccuracy) },
            speed: location.map { ProbeRecord.Speed(metersPerSecond: $0.speed) } ?? .unknown,
            batteryPct: level < 0 ? 0 : UInt8(min(100, (level * 100).rounded())),
            charging: device.batteryState == .charging || device.batteryState == .full,
            lowPower: ProcessInfo.processInfo.isLowPowerModeEnabled,
            prevSendMs: prevSendMs,
            prevSendFailures: prevSendFailures,
            auth: Self.auth(manager.authorizationStatus),
            precise: manager.accuracyAuthorization == .fullAccuracy,
            bgRefresh: Self.bgRefresh(app.backgroundRefreshStatus),
            eventsSinceLast: eventsSinceLast
        )
        eventsSinceLast = 0
        return record
    }

    /// Отправляет очередь по порядку. Отправленное удаляется по идентификатору,
    /// поэтому записи, добавленные во время отправки, не теряются; они уйдут в
    /// следующем круге той же фоновой задачи.
    private func flush() {
        guard !sending else { return }
        guard let token = Keychain.load(account: Keys.token), !token.isEmpty else {
            lastStatus = "нет токена"
            return
        }
        sending = true
        lastSendAt = Date()
        // После фонового пробуждения у приложения несколько секунд: просим ещё.
        let app = UIApplication.shared
        var task = UIBackgroundTaskIdentifier.invalid
        task = app.beginBackgroundTask(withName: "staya.probe") {
            MainActor.assumeIsolated { app.endBackgroundTask(task) }
        }
        Task { @MainActor in
            defer {
                sending = false
                queued = queue.count
                app.endBackgroundTask(task)
            }
            // Не больше нескольких записей за пробуждение — окно короткое.
            var budget = 8
            while budget > 0, let item = queue.peek(1).first {
                budget -= 1
                let result = await ProbeClient.send(item.record, token: token)
                lastStatus = result.status.map { "HTTP \($0)" } ?? "нет ответа"
                if result.ok {
                    prevSendMs = result.milliseconds
                    prevSendFailures = 0
                    queue.remove(id: item.id)
                    sentTotal += 1
                    defaults.set(sentTotal, forKey: Keys.sent)
                } else {
                    prevSendMs = nil
                    prevSendFailures += 1
                    failedTotal += 1
                    defaults.set(failedTotal, forKey: Keys.failed)
                    // 4xx (кроме 401) — запись не примут никогда; не держим её в начале очереди.
                    if let s = result.status, (400..<500).contains(s), s != 401 {
                        queue.remove(id: item.id)
                        continue
                    }
                    break
                }
            }
        }
    }

    private static var queueURL: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("probe-queue.json")
    }

    // MARK: Преобразования

    private static func auth(_ s: CLAuthorizationStatus) -> ProbeRecord.Auth {
        switch s {
        case .authorizedAlways: .always
        case .authorizedWhenInUse: .whenInUse
        case .denied: .denied
        case .restricted: .restricted
        case .notDetermined: .notDetermined
        @unknown default: .notDetermined
        }
    }

    private static func bgRefresh(_ s: UIBackgroundRefreshStatus) -> ProbeRecord.BgRefresh {
        switch s {
        case .available: .available
        case .denied: .denied
        case .restricted: .restricted
        @unknown default: .restricted
        }
    }

    private enum Keys {
        static let strategy = "probe.strategy"
        static let running = "probe.running"
        static let device = "probe.device"
        static let events = "probe.events"
        static let sent = "probe.sent"
        static let failed = "probe.failed"
        /// Аккаунт в Keychain, не ключ UserDefaults.
        static let token = "probe-token"
    }

    func saveToken(_ token: String) -> Bool {
        Keychain.save(token.trimmingCharacters(in: .whitespacesAndNewlines), account: Keys.token)
    }

    var hasToken: Bool {
        !(Keychain.load(account: Keys.token) ?? "").isEmpty
    }

    var backgroundRefresh: String {
        Self.bgRefresh(UIApplication.shared.backgroundRefreshStatus).rawValue
    }
}

// MARK: - CLLocationManagerDelegate

extension LocationEngine: CLLocationManagerDelegate {
    // Менеджер создан на главном потоке, поэтому колбэки приходят на него же.
    // Внутри используем собственный `manager` движка: параметр не Sendable.

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        MainActor.assumeIsolated {
            let status = self.manager.authorizationStatus
            // iOS присылает этот колбэк и при создании менеджера, без изменений:
            // перезапускаем службы только при настоящей смене разрешения.
            let changed = lastAuth != nil && lastAuth != status
            lastAuth = status
            authorization = status
            precise = self.manager.accuracyAuthorization == .fullAccuracy
            // После «при использовании» сразу просим «всегда» — без него фона не будет.
            if status == .authorizedWhenInUse { self.manager.requestAlwaysAuthorization() }
            if changed, running {
                expectStartFix = isActive
                stopAll()
                apply(strategy)
            }
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        MainActor.assumeIsolated {
            guard let last = locations.last else { return }
            let trigger: ProbeRecord.Trigger
            if expectStartFix {
                // Ответ на явный запуск из экрана — не пробуждение системой.
                trigger = .foreground
                expectStartFix = false
            } else {
                // Без непрерывных обновлений сюда приходят именно значимые изменения.
                trigger = continuousOn ? .continuous : .significantChange
            }
            record(trigger, location: last)
            checkMotion()
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didVisit visit: CLVisit) {
        // От визита берём только точность; координаты визита не используются.
        let accuracy = ProbeRecord.accuracy(visit.horizontalAccuracy)
        MainActor.assumeIsolated {
            record(.visit, location: nil, accuracy: accuracy)
            checkMotion()
        }
    }

    /// S4: iOS поставила обновления на паузу, потому что телефон в покое.
    nonisolated func locationManagerDidPauseLocationUpdates(_ manager: CLLocationManager) {
        MainActor.assumeIsolated {
            guard strategy == .s4 else { return }
            stopContinuous(record: true)
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        MainActor.assumeIsolated {
            lastStatus = "ошибка геолокации"
        }
    }
}
#endif
