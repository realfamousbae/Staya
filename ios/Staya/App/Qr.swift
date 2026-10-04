import CoreImage
import CoreImage.CIFilterBuiltins
#if canImport(UIKit)
import UIKit
#endif

/// QR-коды приглашений. Коррекция M — на ней основана граница длины приглашения
/// (proto: версия 13 вмещает 331 байт и уверенно сканируется с экрана).
enum Qr {
    /// Модуль QR — `scale` пикселей (без сглаживания при растяжении).
    static func ciImage(_ text: String, scale: CGFloat = 8) -> CIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        return filter.outputImage?.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
    }

    #if canImport(UIKit)
    static func image(_ text: String, scale: CGFloat = 8) -> UIImage? {
        guard let output = ciImage(text, scale: scale),
              let cg = CIContext().createCGImage(output, from: output.extent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
    #endif
}

/// Ссылка, открытая в приложении: из камеры, буфера или другого приложения.
enum DeepLink: Equatable, Sendable {
    /// `staya://add?…` — приглашение друга. Только экран подтверждения.
    case invite(String)
    /// `staya://server?…` — ссылка на сервер для онбординга.
    case server(String)

    var uri: String {
        switch self {
        case .invite(let u), .server(let u): u
        }
    }

    static func parse(_ text: String?) -> DeepLink? {
        guard let t = text?.trimmingCharacters(in: .whitespacesAndNewlines) else { return nil }
        if t.hasPrefix("staya://add?") { return .invite(t) }
        if t.hasPrefix("staya://server?") { return .server(t) }
        return nil
    }
}
