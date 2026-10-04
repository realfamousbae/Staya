import CoreImage
import Foundation

// QR приглашения тем же генератором, что в приложении (Qr.ciImage), → распознавание.
// Худший случай длины: длинное имя sslip.io с портом и два отпечатка (~280 байт).

func check(_ ok: Bool, _ name: String) {
    guard ok else { print("FAIL: \(name)"); exit(1) }
    print("ok: \(name)")
}

let b64 = String(repeating: "A", count: 43)
let invite = "staya://add?v=1&s=255-255-255-255.sslip.io:8443&p=\(b64).\(b64)&id=\(String(repeating: "B", count: 22))"
    + "&ik=\(b64)&sk=\(b64)&t=\(String(repeating: "C", count: 22))&m=q"
check(invite.utf8.count <= 331, "invite fits the 331-byte bound (\(invite.utf8.count) bytes)")

let image = Qr.ciImage(invite)!
// Версия QR по размеру: модулей = 17 + 4 × версия, плюс поле в 1 модуль с каждой
// стороны у CIQRCodeGenerator (проверено: 213 байт при M — 59 модулей, версия 10).
let modules = Int(image.extent.width / 8)
let version = (modules - 2 - 17) / 4
check(version <= 13, "QR version \(version) ≤ 13 at correction M")

let detector = CIDetector(ofType: CIDetectorTypeQRCode, context: nil, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])!
// Белое поле вокруг кода, как на экране.
let framed = image.transformed(by: CGAffineTransform(translationX: 40, y: 40))
    .composited(over: CIImage(color: .white).cropped(to: image.extent.insetBy(dx: -40, dy: -40).offsetBy(dx: 40, dy: 40)))
let found = detector.features(in: framed).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
check(found == [invite], "decodes back to the same invite")

check(DeepLink.parse(" \(invite) ") == .invite(invite), "invite link is classified")
check(DeepLink.parse("staya://server?v=1&s=a.example") == .server("staya://server?v=1&s=a.example"), "server link")
check(DeepLink.parse("https://example.com") == nil, "other links are ignored")
print("Qr: all passed")
