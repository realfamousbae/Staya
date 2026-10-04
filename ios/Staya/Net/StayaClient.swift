import Foundation
import StayaCore

/// Ошибки сетевого слоя (как в Android `StayaClient`).
enum StayaNetError: Error, Equatable, CustomStringConvertible {
    /// Ключ сервера не совпал с отпечатками или запомненным: подключение запрещено (§5.3).
    case serverKeyRejected
    /// Закрытый сервер: для регистрации нужен код приглашения.
    case inviteCodeRequired
    /// Сервер ограничил частоту запросов (429).
    case rateLimited
    case status(Int, String)
    case noServer

    var description: String {
        switch self {
        case .serverKeyRejected: "server key rejected"
        case .inviteCodeRequired: "invite code required"
        case .rateLimited: "rate limited"
        case .status(let code, let path): "\(path): HTTP \(code)"
        case .noServer: "no server"
        }
    }
}

/// HTTP и WebSocket к серверу Staya (protocol §4, §8.3) поверх `URLSession`.
///
/// Проверка ключа сервера — в делегате на `NSURLAuthenticationMethodServerTrust`,
/// во время рукопожатия и до первого байта запроса (`ServerTrustEvaluator` +
/// `check_server_key` ядра). Вход — как на Android: токен из ядра; 401 — войти и
/// повторить один раз; 404 на challenge — регистрация теми же ключами; 403 — нужен
/// код приглашения; 429 не повторяется.
final class StayaClient: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    private let core: StayaCore
    private let base: URL
    /// Имя сервера для проверки сертификата (без порта).
    private let tlsName: String
    private let inviteCode: @Sendable () -> String?
    private var session: URLSession!
    private let lock = NSLock()
    private var rejectedTasks = Set<Int>()

    /// `baseURL` — явно только для отладки (симулятор → `http://127.0.0.1:…`).
    init(core: StayaCore, baseURL: URL? = nil, inviteCode: @escaping @Sendable () -> String? = { nil }) throws {
        guard let server = try core.server() else { throw StayaNetError.noServer }
        self.core = core
        guard let base = baseURL ?? URL(string: "https://\(server.host)") else { throw StayaNetError.noServer }
        self.base = base
        self.tlsName = server.host.split(separator: ":").first.map(String.init) ?? server.host
        self.inviteCode = inviteCode
        super.init()
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 30
        config.urlCache = nil
        config.httpCookieStorage = nil
        session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }

    deinit { session.invalidateAndCancel() }

    private var now: Int64 { Int64(Date().timeIntervalSince1970) }

    func get(_ path: String) async throws -> String { try await authed { try await self.call("GET", path, nil, $0) } }
    func post(_ path: String, _ json: String) async throws -> String { try await authed { try await self.call("POST", path, json, $0) } }
    func put(_ path: String, _ json: String) async throws -> String { try await authed { try await self.call("PUT", path, json, $0) } }
    func delete(_ path: String) async throws -> String { try await authed { try await self.call("DELETE", path, nil, $0) } }

    /// WebSocket `/v1/ws` с текущим токеном (при необходимости сначала вход).
    func webSocket() async throws -> URLSessionWebSocketTask {
        let token = try await sessionToken()
        var components = URLComponents(url: base.appendingPathComponent("v1/ws"), resolvingAgainstBaseURL: false)!
        components.scheme = base.scheme == "https" ? "wss" : "ws"
        var request = URLRequest(url: components.url!)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        let task = session.webSocketTask(with: request)
        task.maximumMessageSize = 64 * 1024
        return task
    }

    /// Задача отвергнута из-за ключа сервера (для WebSocket: ошибку видит вызывающий).
    func wasRejected(_ task: URLSessionTask) -> Bool {
        lock.withLock { rejectedTasks.contains(task.taskIdentifier) }
    }

    func sessionToken() async throws -> String {
        if let token = try core.sessionToken(now: now) { return token.base64EncodedString() }
        return try await login()
    }

    private func authed(_ block: @Sendable (String) async throws -> String) async throws -> String {
        let token = try await sessionToken()
        do {
            return try await block(token)
        } catch StayaNetError.status(401, _) {
            // Токен отозван или сервер потерял базу: войти заново и повторить один раз.
            try core.clearSession()
            return try await block(try await login())
        }
    }

    private func login() async throws -> String {
        let challenge: String
        do {
            challenge = try await call("POST", "/v1/auth/challenge", try core.authChallengeRequest(), nil)
        } catch StayaNetError.status(404, _) {
            try await register()
            challenge = try await call("POST", "/v1/auth/challenge", try core.authChallengeRequest(), nil)
        }
        let verified = try await call("POST", "/v1/auth/verify", try core.authVerifyRequest(challengeResponseJson: challenge), nil)
        try core.completeLogin(verifyResponseJson: verified)
        guard let token = try core.sessionToken(now: now) else { throw StayaNetError.status(0, "login") }
        return token.base64EncodedString()
    }

    private func register() async throws {
        do {
            _ = try await call("POST", "/v1/accounts", try core.registerRequest(inviteCode: inviteCode()), nil)
        } catch StayaNetError.status(403, _) {
            throw StayaNetError.inviteCodeRequired
        }
    }

    private func call(_ method: String, _ path: String, _ json: String?, _ token: String?) async throws -> String {
        var request = URLRequest(url: base.appendingPathComponent(String(path.dropFirst())))
        request.httpMethod = method
        if let token { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let json {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = Data(json.utf8)
        }
        // Свой делегат на запрос: знать, что именно этот запрос отвергнут из-за ключа.
        let recorder = TrustRecorder(client: self)
        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: request, delegate: recorder)
        } catch {
            if recorder.rejected { throw StayaNetError.serverKeyRejected }
            throw error
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        switch status {
        case 200..<300: return String(decoding: data, as: UTF8.self)
        case 429: throw StayaNetError.rateLimited
        default: throw StayaNetError.status(status, path)
        }
    }

    // MARK: - Проверка сервера

    /// Решение по запросу проверки сервера; `rejected` — отвергнут именно ключ.
    fileprivate func evaluate(_ challenge: URLAuthenticationChallenge)
        -> (URLSession.AuthChallengeDisposition, URLCredential?, rejected: Bool)
    {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust
        else { return (.performDefaultHandling, nil, false) }
        let core = self.core
        let decision = ServerTrustEvaluator.evaluate(trust, host: tlsName) { spki in
            (try? core.checkServerKey(spkiSha256: spki)).map { $0 != .rejected } ?? false
        }
        switch decision {
        case .trusted: return (.useCredential, URLCredential(trust: trust), false)
        case .keyRejected: return (.cancelAuthenticationChallenge, nil, true)
        case .invalidCertificate: return (.cancelAuthenticationChallenge, nil, false)
        }
    }

    /// Для задач без своего делегата (WebSocket).
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        let (disposition, credential, rejected) = evaluate(challenge)
        if rejected { lock.withLock { _ = rejectedTasks.insert(task.taskIdentifier) } }
        completionHandler(disposition, credential)
    }
}

/// Делегат одного HTTP-запроса: та же проверка и отметка об отвергнутом ключе.
private final class TrustRecorder: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    private let client: StayaClient
    private let lock = NSLock()
    private var _rejected = false

    init(client: StayaClient) { self.client = client }

    var rejected: Bool { lock.withLock { _rejected } }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        let (disposition, credential, rejected) = client.evaluate(challenge)
        if rejected { lock.withLock { _rejected = true } }
        completionHandler(disposition, credential)
    }
}
