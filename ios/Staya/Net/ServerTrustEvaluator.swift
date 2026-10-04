import CryptoKit
import Foundation
import Security

/// Правило доверия к серверу (protocol §5.3) для `URLSession`: вызывается из
/// делегата на запрос `NSURLAuthenticationMethodServerTrust` — во время TLS-рукопожатия,
/// до отправки первого байта запроса, одинаково для HTTP и WebSocket.
///
/// 1. Обычная проверка цепочки для имени сервера (а не адреса, к которому подключились).
/// 2. SHA-256 от SubjectPublicKeyInfo ключа сервера — в `check` (ядро сверяет с
///    отпечатками или ключом, запомненным при первом подключении).
///
/// Без зависимостей от ядра: проверяется на Mac тестовыми сертификатами
/// (ios/CoreTests/Trust).
enum ServerTrustEvaluator {
    enum Decision: Equatable {
        case trusted
        /// Цепочка или имя не прошли обычную проверку.
        case invalidCertificate
        /// Ключ не совпал с отпечатками или запомненным (§5.3).
        case keyRejected
    }

    static func evaluate(
        _ trust: SecTrust,
        host: String,
        check: (Data) -> Bool
    ) -> Decision {
        SecTrustSetPolicies(trust, SecPolicyCreateSSL(true, host as CFString))
        var error: CFError?
        guard SecTrustEvaluateWithError(trust, &error),
              let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let leaf = chain.first,
              let spki = subjectPublicKeyInfo(of: leaf)
        else { return .invalidCertificate }
        return check(Data(SHA256.hash(data: spki))) ? .trusted : .keyRejected
    }

    /// DER SubjectPublicKeyInfo из сертификата X.509. Security на iOS отдаёт ключ
    /// только в «сыром» виде, а отпечаток по протоколу — от SPKI целиком, поэтому
    /// элемент берётся из DER сертификата как есть (любой тип ключа).
    static func subjectPublicKeyInfo(of certificate: SecCertificate) -> Data? {
        let der = [UInt8](SecCertificateCopyData(certificate) as Data)
        // Certificate ::= SEQUENCE { tbsCertificate SEQUENCE { [0] version?, serial,
        //   signature, issuer, validity, subject, subjectPublicKeyInfo, ... } ... }
        guard let cert = element(der, at: 0), der[0] == 0x30,
              let tbs = element(der, at: cert.contentStart), der[cert.contentStart] == 0x30
        else { return nil }
        var offset = tbs.contentStart
        // Необязательная версия [0].
        if der[offset] == 0xA0, let v = element(der, at: offset) { offset = v.end }
        // serial, signature, issuer, validity, subject.
        for _ in 0..<5 {
            guard let e = element(der, at: offset) else { return nil }
            offset = e.end
        }
        guard let spki = element(der, at: offset), der[offset] == 0x30, spki.end <= tbs.end else { return nil }
        return Data(der[offset..<spki.end])
    }

    /// Границы элемента DER (только определённая длина, как требует DER).
    private static func element(_ der: [UInt8], at start: Int) -> (contentStart: Int, end: Int)? {
        guard start + 1 < der.count else { return nil }
        let first = Int(der[start + 1])
        var contentStart = start + 2
        var length = first
        if first & 0x80 != 0 {
            let count = first & 0x7F
            guard (1...4).contains(count), contentStart + count <= der.count else { return nil }
            length = 0
            for i in 0..<count { length = length << 8 | Int(der[contentStart + i]) }
            contentStart += count
        }
        let end = contentStart + length
        guard end <= der.count else { return nil }
        return (contentStart, end)
    }
}
