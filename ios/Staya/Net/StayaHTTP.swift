import Foundation

/// Ответ сервера с кодом не 2xx.
struct HTTPStatusError: Error, CustomStringConvertible {
    let status: Int
    let path: String
    var description: String { "\(path): HTTP \(status)" }
}

/// HTTP к серверу Staya (protocol §8.3): JSON и `Authorization: Bearer`. Пока без
/// pinning: задача 2.10 ходит только на dev-сервер по loopback; TLS с pinning — 4.1.
struct StayaHTTP: Sendable {
    let base: URL
    let token: String

    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 15
        config.urlCache = nil
        return URLSession(configuration: config)
    }()

    func get(_ path: String) async throws -> String { try await call("GET", path, nil) }
    func post(_ path: String, _ json: String) async throws -> String { try await call("POST", path, json) }
    func put(_ path: String, _ json: String) async throws -> String { try await call("PUT", path, json) }
    func delete(_ path: String) async throws -> String { try await call("DELETE", path, nil) }

    /// Без авторизации: место встречи `/dev/invite` dev-сервера.
    func getPublic(_ path: String) async throws -> String { try await call("GET", path, nil, auth: false) }

    private func call(_ method: String, _ path: String, _ body: String?, auth: Bool = true) async throws -> String {
        guard let url = URL(string: base.absoluteString.trimmingCharacters(in: ["/"]) + path) else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        if auth { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = Data(body.utf8)
        }
        let (data, response) = try await Self.session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else { throw HTTPStatusError(status: status, path: path) }
        return String(decoding: data, as: UTF8.self)
    }
}
