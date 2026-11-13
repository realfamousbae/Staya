import LocalAuthentication
import Observation
import SwiftUI
import UIKit

/// Screen access only. Keychain accessibility and background location stay unchanged.
@MainActor
@Observable
final class AppLock {
    static let shared = AppLock()
    private(set) var enabled: Bool
    private(set) var locked: Bool
    private(set) var busy = false
    private(set) var message: String?
    private var attempts = UnlockAttempt()
    private var context: LAContext?
    private var covers: [UIView] = []

    private init() {
        let preference = UserDefaults.standard.bool(forKey: "appLockEnabled")
        enabled = preference
        locked = preference
    }

    func background() {
        attempts.invalidate()
        context?.invalidate()
        context = nil
        busy = false
        locked = enabled
    }

    func unlock() { authenticate(newEnabled: nil) }
    func change() { authenticate(newEnabled: !enabled) }

    private func authenticate(newEnabled: Bool?) {
        guard !busy else { return }
        let auth = LAContext()
        var error: NSError?
        guard auth.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            message = "Сначала настрой блокировку экрана в настройках телефона."
            return
        }
        let attempt = attempts.begin()
        context = auth
        busy = true
        message = nil
        auth.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: "Открыть Staya") { success, _ in
            Task { @MainActor in
                let lock = AppLock.shared
                guard lock.attempts.finish(attempt) else { return }
                lock.busy = false
                lock.context = nil
                guard success else {
                    lock.message = "Вход не подтверждён. Можно попробовать ещё раз."
                    return
                }
                if let newEnabled {
                    // Only the preference is stored here; keys remain in Keychain.
                    UserDefaults.standard.set(newEnabled, forKey: "appLockEnabled")
                    lock.enabled = newEnabled
                }
                lock.locked = false
            }
        }
    }

    /// Cover every application window, including presented sheets, before snapshots.
    func coverWindows() {
        guard enabled, covers.isEmpty else { return }
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for window in scene.windows {
                let cover = UIView(frame: window.bounds)
                cover.backgroundColor = UIColor(red: 0.059, green: 0.059, blue: 0.071, alpha: 1)
                cover.autoresizingMask = [.flexibleWidth, .flexibleHeight]
                cover.isUserInteractionEnabled = false
                window.addSubview(cover)
                covers.append(cover)
            }
        }
    }

    func uncoverWindows() {
        covers.forEach { $0.removeFromSuperview() }
        covers.removeAll()
    }
}

struct AppLockScreen: View {
    @State private var lock = AppLock.shared
    var body: some View {
        DarkScreen(title: "Staya заблокирована") {
            if let message = lock.message { Text(message).foregroundStyle(Ink.mist) }
            Button("Открыть") { lock.unlock() }
                .disabled(lock.busy)
                .buttonStyle(.borderedProminent)
        }
    }
}
