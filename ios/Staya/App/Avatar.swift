import UIKit

/// Аватар для профиля: квадрат 128×128 в JPEG не больше 8 КБ (protocol §6,
/// `AVATAR_MAX_LEN`) — профиль уходит друзьям в одном управляющем сообщении.
enum Avatar {
    static let maxBytes = 8192
    static let side: CGFloat = 128

    static func encode(_ image: UIImage) -> Data? {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let size = CGSize(width: side, height: side)
        let square = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            // Заполнить квадрат, обрезав лишнее по центру.
            let scale = max(side / image.size.width, side / image.size.height)
            let w = image.size.width * scale, h = image.size.height * scale
            image.draw(in: CGRect(x: (side - w) / 2, y: (side - h) / 2, width: w, height: h))
        }
        for quality in stride(from: 0.8, through: 0.1, by: -0.1) {
            if let data = square.jpegData(compressionQuality: quality), data.count <= maxBytes {
                return data
            }
        }
        return nil
    }
}
