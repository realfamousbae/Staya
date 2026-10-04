import StayaCore
import SwiftUI

/// Корень приложения: ядро открывается лениво, затем онбординг или главный экран.
/// В сборке прототипа замеров (STAYA_PROBE) экран замеров доступен кнопкой — замеры
/// этапа 1 не зависят от аккаунта.
struct ContentView: View {
    @State private var core = AppCore.shared
    @State private var app = AppModel.shared
    @State private var showProbe = false
    @State private var friends = FriendsModel.shared
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack {
            content
                .toolbar {
                    #if STAYA_PROBE
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Замеры") { showProbe = true }
                    }
                    #endif
                }
        }
        #if STAYA_PROBE
        .sheet(isPresented: $showProbe) { ProbeDebugView() }
        #endif
        .task { core.open() }
        // Ссылки staya://… из других приложений: только в поле онбординга или на подтверждение.
        .onOpenURL { url in app.pendingLink = DeepLink.parse(url.absoluteString) }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { friends.stop() }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch core.state {
        case .open(let staya, let accountId):
            Group {
                switch app.phase {
                case .loading:
                    ProgressView()
                case .onboarding:
                    OnboardingView(core: staya, accountId: accountId)
                case .ready(let nick, let server, let id):
                    HomeView(nick: nick, server: server, accountId: id)
                        // На экране — синхронизация и WebSocket; при уходе в фон — стоп.
                        .task(id: scenePhase) { if scenePhase == .active { friends.start(core: staya) } }
                        .task(id: app.pendingLink) {
                            if let link = app.pendingLink {
                                app.pendingLink = nil
                                friends.open(link)
                            }
                        }
                }
            }
            .task { app.refresh(core: staya, accountId: accountId) }
        case .closed, .opening:
            ProgressView()
        case .unavailable(let reason):
            ContentUnavailableView("Не удалось открыть данные", systemImage: "lock", description: Text(reason))
        case .broken(let reason):
            VStack(spacing: 12) {
                ContentUnavailableView("Данные повреждены", systemImage: "exclamationmark.triangle", description: Text(reason))
                Button("Сбросить локальные данные", role: .destructive) { core.reset() }
            }
        }
    }
}

#Preview {
    ContentView()
}
