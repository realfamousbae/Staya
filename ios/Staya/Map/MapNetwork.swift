import Foundation
import MapLibre
import StayaCore

/// Сеть карты: все запросы MapLibre (стиль, тайлы, шрифты, спрайты) — только https
/// к серверу, к которому привязан аккаунт, и с той же проверкой ключа сервера, что
/// у API (protocol §5.3). Иначе подменивший сервер по дороге видел бы, какой участок
/// карты смотрят — то есть примерно где друзья.
///
/// MapLibre берёт сессию у делегата на каждый запрос, а `delegate` у него `weak`:
/// объект держится в `shared`, иначе MapLibre молча вернулся бы к своей сессии без
/// проверки ключа. Журналы MapLibre выключены: адреса тайлов выдают место.
final class MapNetwork: NSObject, MLNNetworkConfigurationDelegate, @unchecked Sendable {
    static let shared = MapNetwork()

    private let lock = NSLock()
    private var core: StayaCore?
    private var session: URLSession?

    /// До создания первого `MLNMapView`.
    @MainActor
    func install(core: StayaCore) {
        lock.withLock {
            guard self.core !== core else { return }
            self.core = core
            session?.invalidateAndCancel()
            let config = URLSessionConfiguration.ephemeral
            config.timeoutIntervalForResource = 30
            config.httpCookieStorage = nil
            // Делегат только уровня сессии: MapLibre не поддерживает URLSessionDataDelegate.
            session = URLSession(configuration: config, delegate: SessionTrust(network: self), delegateQueue: nil)
        }
        // Уровень .none и пустой обработчик: сообщения ядра MapLibre (в т. ч. адреса тайлов)
        // идут через эту конфигурацию, а не напрямую в NSLog.
        MLNLoggingConfiguration.shared.loggingLevel = .none
        MLNLoggingConfiguration.shared.handler = { _, _, _, _ in }
        MLNNetworkConfiguration.sharedManager.delegate = self
    }

    /// Имя сервера из привязки ядра (на каждый запрос: до первого друга её можно сменить).
    var host: String? {
        let core = lock.withLock { self.core }
        return (try? core?.server())??.host
    }

    func styleURL(dark: Bool) -> URL? {
        guard let host else { return nil }
        return URL(string: "https://\(host)/map/style-\(dark ? "dark" : "light").json")
    }

    /// Границы региона карты из TileJSON сервера.
    func regionBounds() async -> (west: Double, south: Double, east: Double, north: Double)? {
        guard let host, let url = URL(string: "https://\(host)/tiles/region.json"),
              let session = lock.withLock({ self.session }),
              let (data, response) = try? await session.data(from: url),
              (response as? HTTPURLResponse)?.statusCode == 200
        else { return nil }
        return MapMath.tileJsonBounds(data)
    }

    // MARK: MLNNetworkConfigurationDelegate

    // Селекторы явно: метод протокола необязательный, опечатка в имени молча
    // вернула бы MapLibre к сессии без проверки ключа.
    @objc(sessionForNetworkConfiguration:)
    func session(for configuration: MLNNetworkConfiguration) -> URLSession {
        lock.withLock { session } ?? Self.rejectAll
    }

    /// Запасная сессия до `install`: не пропускает ни одного TLS-соединения.
    private static let rejectAll = URLSession(configuration: .ephemeral, delegate: RejectAll(), delegateQueue: nil)

    @objc(willSendRequest:)
    func willSend(_ request: NSMutableURLRequest) -> NSMutableURLRequest {
        // Чужой адрес или http не уходит в сеть: такой URL сессия отклонит сразу.
        if !MapMath.allowed(request.url, host: host) {
            request.url = URL(string: "about:blank")
        }
        return request
    }

    fileprivate func evaluate(_ challenge: URLAuthenticationChallenge) -> (URLSession.AuthChallengeDisposition, URLCredential?) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust
        else { return (.performDefaultHandling, nil) }
        let space = challenge.protectionSpace
        guard let core = lock.withLock({ self.core }),
              let host,
              MapMath.allowed(URL(string: "https://\(space.host):\(space.port)/"), host: host)
        else { return (.cancelAuthenticationChallenge, nil) }
        let tlsName = host.split(separator: ":").first.map(String.init) ?? host
        let decision = ServerTrustEvaluator.evaluate(trust, host: tlsName) { spki in
            (try? core.checkServerKey(spkiSha256: spki)).map { $0 != .rejected } ?? false
        }
        return decision == .trusted ? (.useCredential, URLCredential(trust: trust)) : (.cancelAuthenticationChallenge, nil)
    }
}

/// Проверка ключа на уровне сессии (MapLibre создаёт задачи с completion handler).
private final class SessionTrust: NSObject, URLSessionDelegate, @unchecked Sendable {
    // Сессия держит делегат сильно; сам `MapNetwork.shared` живёт всегда.
    private unowned let network: MapNetwork

    init(network: MapNetwork) { self.network = network }

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        let (disposition, credential) = network.evaluate(challenge)
        completionHandler(disposition, credential)
    }
}

private final class RejectAll: NSObject, URLSessionDelegate {
    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        completionHandler(.cancelAuthenticationChallenge, nil)
    }
}
