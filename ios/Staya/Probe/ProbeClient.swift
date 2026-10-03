#if STAYA_PROBE
import Foundation

/// Отправка метрики на сборщик. Обычная сессия с коротким таймаутом: после
/// фонового пробуждения у приложения около 10 секунд.
enum ProbeClient {
    static let endpoint = URL(string: "https://2-27-42-60.sslip.io/probe")!

    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 8
        config.timeoutIntervalForResource = 8
        config.waitsForConnectivity = false
        return URLSession(configuration: config)
    }()

    struct Result: Sendable {
        /// HTTP-код или `nil`, если ответа не было.
        let status: Int?
        let milliseconds: UInt32
        var ok: Bool { status == 204 }
    }

    static func send(_ record: ProbeRecord, token: String) async -> Result {
        let start = Date()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.httpBody = try? JSONEncoder().encode(record)
        let status: Int?
        do {
            let (_, response) = try await session.data(for: request)
            status = (response as? HTTPURLResponse)?.statusCode
        } catch {
            status = nil
        }
        let ms = UInt32(min(Date().timeIntervalSince(start) * 1000, Double(UInt32.max)))
        return Result(status: status, milliseconds: ms)
    }
}
#endif
