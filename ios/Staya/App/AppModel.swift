import Foundation
import Observation
import StayaCore

/// Что показывать: онбординг или главный экран (задача 4.2). Сеть и ядро — не на
/// главном потоке; состояние — на нём.
@MainActor
@Observable
final class AppModel {
    enum Phase {
        case loading
        case onboarding
        case ready(nick: String, server: String, accountId: String)
    }

    static let shared = AppModel()

    private(set) var phase: Phase = .loading
    private(set) var busy = false
    var error: String?

    /// Пересчитать по ядру: есть привязка к серверу и ник — онбординг пройден.
    func refresh(core: StayaCore, accountId: String) {
        let server = try? core.server()
        let nick = (try? core.myProfile().nick) ?? ""
        if let server, !nick.isEmpty {
            phase = .ready(nick: nick, server: server.host, accountId: accountId)
        } else {
            phase = .onboarding
        }
    }

    /// Создание аккаунта на сервере: привязка, профиль, регистрация и вход
    /// (внутри публикации ключей), затем — приглашение друга, если оно было.
    func createAccount(
        core: StayaCore,
        accountId: String,
        link: String,
        manualHost: String,
        inviteCode: String,
        nick: String,
        avatar: Data?
    ) {
        busy = true
        error = nil
        let link = link.trimmingCharacters(in: .whitespacesAndNewlines)
        let host = manualHost.trimmingCharacters(in: .whitespacesAndNewlines)
        let code = inviteCode.trimmingCharacters(in: .whitespacesAndNewlines)
        let nick = nick.trimmingCharacters(in: .whitespacesAndNewlines)
        Task.detached {
            let message: String?
            do {
                if !link.isEmpty {
                    _ = try core.setServerFromLink(uri: link)
                } else if !host.isEmpty {
                    _ = try core.setServer(host: host, pins: [])
                } else {
                    throw OnboardingError.noServer
                }
                try core.setProfile(nick: nick, avatar: avatar ?? Data())
                let client = try StayaClient(core: core, inviteCode: { code.isEmpty ? nil : code })
                let sync = CoreSync(core: core, http: client)
                try await sync.publishKeys()
                if link.hasPrefix("staya://add?") {
                    try await sync.accept(link)
                }
                message = nil
            } catch {
                message = Self.describe(error)
                // Пока друзей нет, неверный адрес можно исправить и попробовать снова.
                try? core.resetServer()
            }
            await MainActor.run {
                self.busy = false
                self.error = message
                if message == nil { self.refresh(core: core, accountId: accountId) }
            }
        }
    }

    enum OnboardingError: Error {
        case noServer
    }

    nonisolated static func describe(_ error: Error) -> String {
        switch error {
        case OnboardingError.noServer:
            return "Вставь приглашение друга или ссылку на сервер — или укажи сервер в «Дополнительно»."
        case StayaNetError.inviteCodeRequired:
            return "Этот сервер закрытый: нужен код приглашения на регистрацию. Его даёт владелец сервера."
        case StayaNetError.serverKeyRejected:
            return "Ключ сервера не совпал с ожидаемым. Возможно, соединение перехватывают — не продолжай и спроси у того, кто дал ссылку."
        case StayaNetError.rateLimited:
            return "Сервер просит подождать. Попробуй через минуту."
        case CoreError.ServerMismatch:
            return "Этот аккаунт уже привязан к другому серверу. Друзья должны быть на одном сервере."
        case CoreError.Proto, CoreError.InvalidInvite:
            return "Не получилось разобрать ссылку. Проверь, что она скопирована целиком."
        case let urlError as URLError:
            return "Нет связи с сервером (\(urlError.code.rawValue)). Проверь адрес и интернет."
        default:
            return "Не получилось: \(error)"
        }
    }
}
