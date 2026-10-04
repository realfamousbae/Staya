import CoreLocation
import CoreMotion
import Foundation
import Observation
import StayaCore
import UIKit

/// «Делиться позицией» (задача 4.5): стратегия S4 из этапа 1
/// (`docs/measurements/stage1.md`) — значимые изменения (SLC) и визиты будят
/// приложение и после выгрузки; при движении по CoreMotion — непрерывные
/// обновления (100 м), в покое iOS ставит их на паузу, и мы их выключаем.
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

    private let manager = CLLocationManager()
    private let motion = CMMotionActivityManager()
    private var throttle = SendThrottle()
    private var lastAuth: CLAuthorizationStatus?

    private static let enabledKey = "location.sharing"

    override private init() {
        enabled = UserDefaults.standard.bool(forKey: Self.enabledKey)
        super.init()
        manager.delegate = self
        authorization = manager.authorizationStatus
    }

    // MARK: Жизненный цикл

    /// Из `AppDelegate.didFinishLaunching` — до какого-либо экрана. Службы
    /// выставляются заново: включено — S4, выключено — ничего, в том числе SLC,
    /// оставшийся от сборок с замерами этапа 1.
    func start() {
        if enabled { applyServices() } else { stopServices() }
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
    }

    /// Выключение — режим призрака: друзья видят «скрыто», а не последнюю точку как текущую.
    func disable(core: StayaCore) {
        enabled = false
        UserDefaults.standard.set(false, forKey: Self.enabledKey)
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
        checkMotion()
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
        manager.pausesLocationUpdatesAutomatically = true
        manager.allowsBackgroundLocationUpdates = true
        manager.showsBackgroundLocationIndicator = true
        manager.startUpdatingLocation()
        continuousOn = true
    }

    private func stopContinuous() {
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
        guard enabled, !continuousOn, CMMotionActivityManager.isActivityAvailable() else { return }
        let since = Date().addingTimeInterval(-5 * 60)
        motion.queryActivityStarting(from: since, to: Date(), to: .main) { activities, _ in
            MainActor.assumeIsolated {
                let moving = (activities ?? []).contains { a in
                    a.confidence != .low && (a.walking || a.running || a.cycling || a.automotive)
                }
                if moving, self.enabled { self.startContinuous() }
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
        guard enabled, LocationPolicy.toCore(fix) != nil, let fix = throttle.offer(fix, now: Date()) else { return }
        send(fix)
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
            _ = try? await StayaNet.shared.queue.run { try await LocationSend.queue(core: core, sync: sync, fix: fix) }
        }
    }
}

/// Замер → ядро → сервер. Общий путь сервиса и самопроверки в CI: сначала пакет
/// надёжно ложится в исходящую очередь ядра, потом — попытка отправки; не ушедшее
/// уйдёт при следующей отправке или открытии приложения.
enum LocationSend {
    /// `true` — пакеты поставлены в очередь. Вызывать внутри `StayaNet.queue`.
    static func queue(core: StayaCore, sync: CoreSync?, fix: Fix) async throws -> Bool {
        guard let p = LocationPolicy.toCore(fix) else { return false }
        let location = Location(latE7: p.latE7, lonE7: p.lonE7, accuracyM: p.accuracyM, timestamp: p.timestamp)
        try core.prepareLocationUpdate(location: location, now: Int64(Date().timeIntervalSince1970))
        try? await sync?.flush()
        return true
    }
}

// MARK: - CLLocationManagerDelegate

extension LocationEngine: CLLocationManagerDelegate {
    // Менеджер создан на главном потоке — колбэки приходят на него же.

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        MainActor.assumeIsolated {
            let status = self.manager.authorizationStatus
            let changed = lastAuth != nil && lastAuth != status
            lastAuth = status
            authorization = status
            // После «при использовании» — сразу «всегда»: без него фона не будет.
            if enabled, status == .authorizedWhenInUse { self.manager.requestAlwaysAuthorization() }
            if changed, enabled { applyServices() }
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
            stopContinuous()
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {}
}
