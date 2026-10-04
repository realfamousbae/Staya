import SwiftUI
import UIKit

@main
struct StayaApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        #if DEBUG
        SelfTest.runIfRequested()
        #endif
        #if STAYA_PROBE
        // Здесь, а не в экране: при фоновом перезапуске по SLC/визиту сцены нет.
        LocationEngine.shared.start(launchedForLocation: launchOptions?[.location] != nil)
        #endif
        return true
    }
}
