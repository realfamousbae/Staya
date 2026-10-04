import Foundation
import Security

// Правило доверия (ServerTrustEvaluator) на тестовых сертификатах из
// scripts/test-ios-trust.sh: тестовый CA и листы для localhost и other.example.

let dir = URL(fileURLWithPath: CommandLine.arguments[1])

func cert(_ name: String) -> SecCertificate {
    let data = try! Data(contentsOf: dir.appendingPathComponent("\(name).der"))
    return SecCertificateCreateWithData(nil, data as CFData)!
}

func pin(_ name: String) -> Data {
    let b64 = try! String(contentsOf: dir.appendingPathComponent("\(name).pin"), encoding: .utf8)
    return Data(base64Encoded: b64.trimmingCharacters(in: .whitespacesAndNewlines))!
}

func trust(_ leaf: String, anchored: Bool = true) -> SecTrust {
    var t: SecTrust?
    SecTrustCreateWithCertificates([cert(leaf), cert("ca")] as CFArray, SecPolicyCreateSSL(true, nil), &t)
    if anchored {
        SecTrustSetAnchorCertificates(t!, [cert("ca")] as CFArray)
        SecTrustSetAnchorCertificatesOnly(t!, true)
    }
    return t!
}

func check(_ ok: Bool, _ name: String) {
    guard ok else { print("FAIL: \(name)"); exit(1) }
    print("ok: \(name)")
}

// SPKI из DER сертификата совпадает с тем, что считает openssl.
for name in ["leaf", "leaf2", "other"] {
    let spki = ServerTrustEvaluator.subjectPublicKeyInfo(of: cert(name))!
    var digest = [UInt8](repeating: 0, count: 32)
    spki.withUnsafeBytes { sha256ForTest($0, digest: &digest) }
    check(Data(digest) == pin(name), "SPKI of \(name) matches openssl")
}

var asked: [Data] = []
let accept: (Data) -> Bool = { asked.append($0); return true }

check(ServerTrustEvaluator.evaluate(trust("leaf"), host: "localhost", check: accept) == .trusted, "valid chain and name")
check(asked == [pin("leaf")], "core gets the SPKI hash of the leaf")

asked = []
check(ServerTrustEvaluator.evaluate(trust("leaf"), host: "localhost", check: { asked.append($0); return false }) == .keyRejected,
      "key rejected by the core")

asked = []
check(ServerTrustEvaluator.evaluate(trust("other"), host: "localhost", check: accept) == .invalidCertificate,
      "name mismatch")
check(asked.isEmpty, "name mismatch never reaches the core (nothing learned)")

asked = []
check(ServerTrustEvaluator.evaluate(trust("leaf", anchored: false), host: "localhost", check: accept) == .invalidCertificate,
      "untrusted chain")
check(asked.isEmpty, "untrusted chain never reaches the core")

check(ServerTrustEvaluator.evaluate(trust("leaf2"), host: "localhost", check: { $0 == pin("leaf") }) == .keyRejected,
      "another key for the same name is rejected")
print("Trust: all passed")
