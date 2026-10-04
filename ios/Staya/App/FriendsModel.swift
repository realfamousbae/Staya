import Foundation
import Observation
import StayaCore

/// Друзья (задача 4.3): приглашения, принятие после подтверждения, код
/// безопасности, живая синхронизация на экране. Сеть с ядром — через одну
/// `SerialQueue` вместе с `LiveConnection`.
@MainActor
@Observable
final class FriendsModel {
    enum Screen: Identifiable {
        case showQr(uri: String, expiresAt: Date)
        case shareLink(String)
        case scan
        case confirm(uri: String, info: InviteInfo)
        case safety(FriendView, code: String)

        var id: String {
            switch self {
            case .showQr(let u, _): "qr" + u
            case .shareLink(let u): "link" + u
            case .scan: "scan"
            case .confirm(let u, _): "confirm" + u
            case .safety(let f, _): "safety" + f.accountId
            }
        }
    }

    static let shared = FriendsModel()

    var screen: Screen?
    private(set) var friends: [FriendView] = []
    private(set) var busy = false
    var message: String?

    private var core: StayaCore?
    private var sync: CoreSync?
    private var live: LiveConnection?
    private let queue = SerialQueue()

    /// Приложение на экране и аккаунт готов: ключи, ящик, WebSocket.
    func start(core: StayaCore) {
        if self.core !== core {
            self.core = core
            guard let client = try? StayaClient(core: core) else { return }
            let sync = CoreSync(core: core, http: client)
            self.sync = sync
            live = LiveConnection(
                client: client, sync: sync, queue: queue,
                onEvents: { _ in Task { @MainActor in FriendsModel.shared.reload() } },
                onError: { error in Task { @MainActor in FriendsModel.shared.message = AppModel.describe(error) } }
            )
        }
        reload()
        guard let sync, let live else { return }
        Task {
            // Пополнить одноразовые ключи (их разбирают при добавлении) и повернуть fallback.
            do { try await queue.run { try await sync.publishKeys() } } catch { message = AppModel.describe(error) }
            await live.start()
        }
    }

    func stop() {
        guard let live else { return }
        Task { await live.stop() }
    }

    func reload() {
        friends = (try? core?.listFriends()) ?? []
    }

    /// Новое приглашение: QR — 10 минут, ссылка — 24 часа (protocol §5).
    func invite(_ method: InviteMethod) {
        guard let core else { return }
        do {
            let uri = try core.createInvite(method: method, now: Int64(Date().timeIntervalSince1970))
            screen = method == .qr ? .showQr(uri: uri, expiresAt: Date().addingTimeInterval(600)) : .shareLink(uri)
        } catch {
            message = AppModel.describe(error)
        }
    }

    /// Ссылка из камеры, буфера или другого приложения: только экран подтверждения.
    func open(_ link: DeepLink) {
        guard let core else { return }
        guard case .invite(let uri) = link else {
            message = "Это ссылка на сервер, а не приглашение. Сервер выбирается один раз — при создании аккаунта."
            return
        }
        do {
            let info = try core.parseInvite(uri: uri)
            if let mine = try core.server()?.host, mine != info.server {
                message = "Этот друг на другом сервере (\(info.server)). Друзья должны быть на одном сервере."
                return
            }
            screen = .confirm(uri: uri, info: info)
        } catch {
            message = "Не получилось разобрать приглашение. Проверь, что ссылка скопирована целиком."
        }
    }

    /// Явное «Добавить» на экране подтверждения.
    func accept(_ uri: String) {
        guard let sync else { return }
        busy = true
        Task {
            do {
                try await queue.run { try await sync.accept(uri) }
                screen = nil
                message = "Запрос отправлен. Друг появится, когда его приложение будет на связи."
            } catch {
                message = AppModel.describe(error)
            }
            busy = false
            reload()
        }
    }

    func showSafety(_ friend: FriendView) {
        guard let core else { return }
        do {
            screen = .safety(friend, code: try core.safetyCode(friend: friend.accountId))
        } catch {
            message = AppModel.describe(error)
        }
    }

    func markVerified(_ friend: FriendView) {
        do { try core?.markVerified(friend: friend.accountId) } catch { message = AppModel.describe(error) }
        reload()
        screen = nil
    }
}
