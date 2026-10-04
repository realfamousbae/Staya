import Foundation
import Security

/// Решение о TLS-соединении карты без MapLibre (проверяется на Mac:
/// scripts/test-ios-trust.sh): только привязанный сервер (имя и порт), затем то же
/// правило ключа, что у API (`ServerTrustEvaluator` → `check_server_key`).
enum MapTrust {
    static func allows(
        _ trust: SecTrust,
        host challengeHost: String,
        port challengePort: Int,
        boundHost: String?,
        check: (Data) -> Bool
    ) -> Bool {
        guard let boundHost,
              MapMath.allowed(URL(string: "https://\(challengeHost):\(challengePort)/"), host: boundHost)
        else { return false }
        let tlsName = boundHost.split(separator: ":").first.map(String.init) ?? boundHost
        return ServerTrustEvaluator.evaluate(trust, host: tlsName, check: check) == .trusted
    }
}
