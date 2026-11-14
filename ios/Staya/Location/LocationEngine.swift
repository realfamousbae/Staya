import CoreLocation
import CoreMotion
import Foundation
import Observation
import StayaCore
import UIKit

/// «Делиться позицией» (задача 4.5): стратегия S4 из этапа 1
/// (`docs/measurements/stage1.md`) с экономным наблюдением в покое. SLC и визиты
/// восстанавливают работу после выгрузки; движение включает точность 100 м.
/// В покое наблюдение продолжается с километровой точностью без автопаузы.
///
/// Создаётся в `AppDelegate`: при фоновом перезапуске по SLC или визиту сцены нет.
/// Включается только явным действием пользователя; флаг «включено» — не секрет.
/// Никогда не печатать `CLLocation` и `CLVisit`: в описании координаты.
@MainActor
@Observable
final class LocationEngine: NSObject {
    static let shared = LocationEngine()

    private(set) var enabled: Bool
    private(set) var authorization: CLAuthorizationStatus = .notDetermined
    private(set) var continuousOn = false
    /// «Точная геопозиция» выключена: iOS отдаёт точки с точностью в километры.
    private(set) var reducedAccuracy = false

    private let manager = CLLocationManager()
    private let motion = CMMotionActivityManager()
    private var throttle = SendThrottle()
    private var remoteRefresh = false
    private var lastMovingFixAt = Date.distantPast
    private var motionMonitor: Task<Void, Never>?
    private var restAnchor: Fix?
    private var lastRestSend: Date?
    private(set) var lastFixAt: Date?
    private(set) var lastSendAt: Date?
    private(set) var lastRequestAt: Date?
    private(set) var lastError: String?
    private var lastMailboxPoll: Date?
    private var lastMotionQuery: Date?
    private var refreshTimeout: Task<Void, Never>?
    private var lastAuth: CLAuthorizationStatus?

    private nonisolated static let enabledKey = "location.sharing"

    override private init() {
        enabled = UserDefaults.standard.bool(forKey: Self.enabledKey)
        super.init()
        manager.delegate = self
        authorization = manager.authorizationStatus
        reducedAccuracy = manager.accuracyAuthorization == .reducedAccuracy
    }

    // MARK: Жизненный цикл

    /// Из `AppDelegate.didFinishLaunching` — до какого-либо экрана. Службы
    /// выставляются заново: включено — S4, выключено — ничего, в том числе SLC,
    /// оставшийся от сборок с замерами этапа 1.
    func start() {
        if enabled { applyServices() } else { stopServices() }
    }

    /// Один свежий замер: при включении передачи, открытии приложения и смене
    /// разрешений. Без него друзьям до следующего пробуждения SLC ушла бы
    /// последняя сохранённая точка — она бывает давно устаревшей.
    nonisolated static func sharingIsEnabled() -> Bool { UserDefaults.standard.bool(forKey: enabledKey) }

    func refresh(remote: Bool = false) {
        guard enabled else { return }
        if remote {
            remoteRefresh = true; lastRequestAt = Date()
            refreshTimeout?.cancel()
            refreshTimeout = Task { @MainActor in
                do { try await Task.sleep(for: .seconds(30)) } catch { return }
                remoteRefresh = false
                lastError = "Не удалось получить свежую позицию за 30 секунд"
                if enabled && !continuousOn { startResting() }
            }
        }
        switch manager.authorizationStatus {
        case .authorizedAlways, .authorizedWhenInUse:
            if !continuousOn { manager.desiredAccuracy = kCLLocationAccuracyHundredMeters }
            manager.requestLocation()
        default:
            break
        }
    }

    /// Явное «Делиться позицией».
    func enable(core: StayaCore) {
        enabled = true
        UserDefaults.standard.set(true, forKey: Self.enabledKey)
        // Первый замер после включения уходит сразу, а не через минуту «скрыто».
        throttle.reset()
        requestAuthorization()
        requestMotionPermission()
        Task {
            _ = try? await StayaNet.shared.queue.run { try core.setGhost(ghost: false) }
            await Self.resend(core: core)
        }
        applyServices()
        refresh()
    }

    /// Выключение — режим призрака: друзья видят «скрыто», а не последнюю точку как текущую.
    func disable(core: StayaCore) {
        enabled = false
        UserDefaults.standard.set(false, forKey: Self.enabledKey)
        refreshTimeout?.cancel()
        remoteRefresh = false
        stopServices()
        let sync = StayaNet.shared.bind(core)?.sync
        Task {
            try? await StayaNet.shared.queue.run {
                try core.setGhost(ghost: true)
                try core.prepareLocationUpdate(location: nil, now: Int64(Date().timeIntervalSince1970))
                try? await sync?.flush()
            }
        }
    }

    func requestAuthorization() {
        switch manager.authorizationStatus {
        case .notDetermined: manager.requestWhenInUseAuthorization()
        case .authorizedWhenInUse: manager.requestAlwaysAuthorization()
        default: break
        }
    }

    /// Пакеты всем друзьям из последнего замера (смена режима или точности, 4.6).
    /// Замера ещё не было — отправлять нечего, это не ошибка.
    static func resend(core: StayaCore) async {
        let sync = StayaNet.shared.bind(core)?.sync
        _ = try? await StayaNet.shared.queue.run {
            try core.prepareLocationUpdate(location: nil, now: Int64(Date().timeIntervalSince1970))
            try? await sync?.flush()
        }
    }

    /// «Заморозить здесь»: друзья видят последнюю точку, пока заморозку не снимут.
    /// `false` — замера ещё не было.
    func setFrozen(_ frozen: Bool, core: StayaCore) async -> Bool {
        let ok = (try? await StayaNet.shared.queue.run {
            if frozen { try core.freezeHere() } else { try core.setFrozen(frozen: nil) }
            return true
        }) ?? false
        if ok { await Self.resend(core: core) }
        return ok
    }

    func isFrozen(core: StayaCore) -> Bool {
        (try? core.sharing().frozen) != nil
    }

    // MARK: Службы

    private func applyServices() {
        // SLC — основа: только он будит приложение после выгрузки.
        manager.startMonitoringSignificantLocationChanges()
        manager.startMonitoringVisits()
        startResting()
        checkMotion()
    }

    private func startResting() {
        motionMonitor?.cancel()
        motionMonitor = nil
        if continuousOn, let held = throttle.takeHeld(now: Date()) { send(held) }
        restAnchor = nil
        manager.desiredAccuracy = kCLLocationAccuracyKilometer
        manager.distanceFilter = kCLDistanceFilterNone
        manager.pausesLocationUpdatesAutomatically = false
        manager.allowsBackgroundLocationUpdates = true
        manager.showsBackgroundLocationIndicator = true
        manager.startUpdatingLocation()
        continuousOn = false
    }

    private func stopServices() {
        manager.stopMonitoringSignificantLocationChanges()
        manager.stopMonitoringVisits()
        stopContinuous()
        motion.stopActivityUpdates()
    }

    private func startContinuous() {
        guard !continuousOn else { return }
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
        manager.distanceFilter = 100
        manager.activityType = .other
        manager.pausesLocationUpdatesAutomatically = false
        manager.allowsBackgroundLocationUpdates = true
        manager.showsBackgroundLocationIndicator = true
        manager.startUpdatingLocation()
        continuousOn = true
        lastMovingFixAt = Date()
        motionMonitor = Task { @MainActor in
            while enabled && continuousOn {
                do { try await Task.sleep(for: .seconds(60)) } catch { return }
                if Date().timeIntervalSince(lastMovingFixAt) >= 600 { startResting(); return }
                checkMotion()
            }
        }
    }

    private func stopContinuous() {
        motionMonitor?.cancel()
        motionMonitor = nil
        guard continuousOn else {
            manager.stopUpdatingLocation()
            return
        }
        manager.stopUpdatingLocation()
        manager.allowsBackgroundLocationUpdates = false
        continuousOn = false
    }

    /// В фоне CoreMotion не присылает обновлений: на каждом пробуждении спрашиваем
    /// недавнюю активность сами.
    private func checkMotion() {
        guard enabled, CMMotionActivityManager.isActivityAvailable() else { return }
        if let lastMotionQuery, Date().timeIntervalSince(lastMotionQuery) < 60 { return }
        lastMotionQuery = Date()
        let since = Date().addingTimeInterval(-5 * 60)
        motion.queryActivityStarting(from: since, to: Date(), to: .main) { activities, _ in
            MainActor.assumeIsolated {
                let moving = (activities ?? []).contains { a in
                    a.confidence != .low && (a.walking || a.running || a.cycling || a.automotive)
                }
                if self.enabled {
                    if moving { self.startContinuous() }
                    else if (activities ?? []).contains(where: { $0.stationary && $0.confidence != .low }) { self.startResting() }
                }
            }
        }
    }

    /// Разрешение на датчик движения можно запросить только с открытым экраном.
    private func requestMotionPermission() {
        guard UIApplication.shared.applicationState == .active, CMMotionActivityManager.isActivityAvailable(),
              CMMotionActivityManager.authorizationStatus() == .notDetermined else { return }
        let now = Date()
        motion.queryActivityStarting(from: now.addingTimeInterval(-60), to: now, to: .main) { _, _ in }
    }

    // MARK: Отправка

    private func handle(_ fix: Fix) {
        guard enabled, LocationPolicy.toCore(fix) != nil, LocationPolicy.isFresh(fix, now: Date()) else { return }
        lastFixAt = fix.timestamp
        if continuousOn { lastMovingFixAt = Date() }
        if !continuousOn {
            if let anchor = restAnchor {
                let from = CLLocation(latitude: anchor.latitude, longitude: anchor.longitude)
                let to = CLLocation(latitude: fix.latitude, longitude: fix.longitude)
                if from.distance(from: to) > 250 + anchor.accuracyM + fix.accuracyM { startContinuous() }
            } else { restAnchor = fix }
        }
        checkMotion()
        if remoteRefresh && fix.timestamp >= (lastRequestAt ?? .distantPast).addingTimeInterval(-5) {
            remoteRefresh = false
            refreshTimeout?.cancel()
            throttle.reset()
            send(fix)
            if !continuousOn { startResting() }
        } else if !continuousOn {
            if let lastRestSend, Date().timeIntervalSince(lastRestSend) < 300 { return }
            lastRestSend = Date()
            send(fix)
        } else if let offered = throttle.offer(fix, now: Date()) { send(offered) }
    }

    private func send(_ fix: Fix) {
        // После фонового пробуждения у приложения несколько секунд: просим ещё.
        let app = UIApplication.shared
        var task = UIBackgroundTaskIdentifier.invalid
        task = app.beginBackgroundTask(withName: "staya.location") {
            MainActor.assumeIsolated { app.endBackgroundTask(task) }
        }
        Task { @MainActor in
            defer { app.endBackgroundTask(task) }
            // Ядро открывается и без экрана; до первой разблокировки ключ недоступен —
            // пропускаем это пробуждение (никакого сброса).
            guard let core = await AppCore.shared.openAndWait() else { return }
            let sync = StayaNet.shared.bind(core)?.sync
            do {
                let queued = try await StayaNet.shared.queue.run { try await LocationSend.queue(core: core, sync: sync, fix: fix) }
                if queued { lastSendAt = Date(); lastError = nil }
            } catch { lastError = "Не удалось отправить позицию" }
            await pollMailbox(core: core, sync: sync)
        }
    }

    /// Ящик на пробуждении геолокации, когда экрана нет (4.9d): заявка друга по моей
    /// ссылке обрабатывается, не дожидаясь, пока я открою приложение (protocol §5).
    /// На экране ящик ведёт WebSocket.
    private func pollMailbox(core: StayaCore, sync: CoreSync?) async {
        guard let sync, UIApplication.shared.applicationState != .active,
              LocationPolicy.shouldPollMailbox(now: Date(), lastPoll: lastMailboxPoll) else { return }
        lastMailboxPoll = Date()
        _ = try? await StayaNet.shared.queue.run {
            let events = try await sync.sync()
            // Новому другу — сразу последний замер, как на экране.
            if events.contains(where: { if case .friendAdded = $0 { true } else { false } }) {
                try core.prepareLocationUpdate(location: nil, now: Int64(Date().timeIntervalSince1970))
                try? await sync.flush()
            }
        }
    }
}

/// Замер → ядро → сервер. Общий путь сервиса и самопроверки в CI: сначала пакет
/// надёжно ложится в исходящую очередь ядра, потом — попытка отправки; не ушедшее
/// уйдёт при следующей отправке или открытии приложения.
enum LocationSend {
    /// `true` — пакеты поставлены в очередь. Вызывать внутри `StayaNet.queue`.
    static func queue(core: StayaCore, sync: CoreSync?, fix: Fix) async throws -> Bool {
        guard let p = LocationPolicy.toCore(fix), LocationPolicy.isFresh(fix, now: Date()) else { return false }
        let location = Location(latE7: p.latE7, lonE7: p.lonE7, accuracyM: p.accuracyM, timestamp: p.timestamp)
        try await DeviceBattery.refresh(core: core)
        try core.prepareLocationUpdate(location: location, now: Int64(Date().timeIntervalSince1970))
        guard let sync else { throw URLError(.notConnectedToInternet) }
        try await sync.flush()
        return true
    }
}

// MARK: - CLLocationManagerDelegate

extension LocationEngine: CLLocationManagerDelegate {
    // Менеджер создан на главном потоке — колбэки приходят на него же.

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        MainActor.assumeIsolated {
            let status = self.manager.authorizationStatus
            let reduced = self.manager.accuracyAuthorization == .reducedAccuracy
            // Колбэк приходит и при создании менеджера — без изменений.
            let changed = lastAuth != nil && (lastAuth != status || reducedAccuracy != reduced)
            lastAuth = status
            authorization = status
            reducedAccuracy = reduced
            // После «при использовании» — сразу «всегда»: без него фона не будет.
            if enabled, status == .authorizedWhenInUse { self.manager.requestAlwaysAuthorization() }
            if changed, enabled {
                applyServices()
                // Включили «Точную геопозицию» или «Всегда» — сразу новая точка.
                refresh()
            }
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let last = locations.last else { return }
        let fix = Fix(latitude: last.coordinate.latitude, longitude: last.coordinate.longitude,
                      accuracyM: last.horizontalAccuracy, timestamp: last.timestamp)
        MainActor.assumeIsolated {
            handle(fix)
            checkMotion()
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didVisit visit: CLVisit) {
        // Только приход: человек сейчас здесь. Уход — старое место, его сообщит SLC.
        guard visit.departureDate == .distantFuture else { return }
        let fix = Fix(latitude: visit.coordinate.latitude, longitude: visit.coordinate.longitude,
                      accuracyM: visit.horizontalAccuracy, timestamp: Date())
        MainActor.assumeIsolated {
            handle(fix)
            checkMotion()
        }
    }

    /// iOS поставила непрерывные обновления на паузу: телефон в покое.
    nonisolated func locationManagerDidPauseLocationUpdates(_ manager: CLLocationManager) {
        MainActor.assumeIsolated {
            if enabled, let held = throttle.takeHeld(now: Date()) { send(held) }
            if enabled { startResting() } else { stopContinuous() }
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        MainActor.assumeIsolated { lastError = "Не удалось получить позицию"; remoteRefresh = false }
    }
}

enum DeviceBattery {
    static func refresh(core: StayaCore) async throws {
        let enabled = try core.batterySharing()
        let reading: (UInt8?, Bool) = await MainActor.run {
            UIDevice.current.isBatteryMonitoringEnabled = enabled
            guard enabled, UIDevice.current.batteryLevel >= 0 else { return (nil, false) }
            let level = min(100, max(0, Int((UIDevice.current.batteryLevel * 100).rounded())))
            let state = UIDevice.current.batteryState
            return (UInt8(level), state == .charging || state == .full)
        }
        try core.setDeviceBattery(percent: reading.0, charging: reading.1)
    }
}
