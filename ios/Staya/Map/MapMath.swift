import Foundation

/// Вспомогательное для карты без MapLibre (проверяется на Mac: ios/CoreTests/Map).
enum MapMath {
    private static let earthRadiusM = 6_371_008.8

    /// Круг точности как многоугольник: (широта, долгота), замкнутый (последняя
    /// точка = первая). Радиус в метрах честный на любом масштабе карты.
    static func accuracyRing(latE7: Int32, lonE7: Int32, radiusM: Double, points: Int = 48) -> [(lat: Double, lon: Double)] {
        let lat = Double(latE7) / 1e7 * .pi / 180
        let lon = Double(lonE7) / 1e7 * .pi / 180
        let d = radiusM / earthRadiusM
        var ring: [(lat: Double, lon: Double)] = (0..<points).map { i in
            let bearing = 2 * Double.pi * Double(i) / Double(points)
            let lat2 = asin(sin(lat) * cos(d) + cos(lat) * sin(d) * cos(bearing))
            let lon2 = lon + atan2(sin(bearing) * sin(d) * cos(lat), cos(d) - sin(lat) * sin(lat2))
            return (lat2 * 180 / .pi, lon2 * 180 / .pi)
        }
        ring.append(ring[0])
        return ring
    }

    /// «только что», «5 мин назад», «3 ч назад», «2 дн назад».
    static func ageLabel(now: Int64, timestamp: Int64) -> String {
        let age = max(0, now - timestamp)
        switch age {
        case ..<60: return "только что"
        case ..<3600: return "\(age / 60) мин назад"
        case ..<86_400: return "\(age / 3600) ч назад"
        default: return "\(age / 86_400) дн назад"
        }
    }

    /// `bounds` из TileJSON сервера карты (`/tiles/region.json`): запад, юг, восток, север.
    static func tileJsonBounds(_ data: Data) -> (west: Double, south: Double, east: Double, north: Double)? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let values = object["bounds"] as? [Any], values.count == 4
        else { return nil }
        let numbers = values.compactMap { ($0 as? NSNumber).flatMap { CFGetTypeID($0) == CFBooleanGetTypeID() ? nil : $0.doubleValue } }
        guard numbers.count == 4 else { return nil }
        let (west, south, east, north) = (numbers[0], numbers[1], numbers[2], numbers[3])
        guard (-180...180).contains(west), (-180...180).contains(east),
              (-90...90).contains(south), (-90...90).contains(north),
              west < east, south < north
        else { return nil }
        return (west, south, east, north)
    }

    /// Запросы карты — только https и только к привязанному серверу (имя и порт).
    static func allowed(_ url: URL?, host: String?) -> Bool {
        guard let url, let host, url.scheme == "https",
              let bound = URL(string: "https://\(host)/")
        else { return false }
        return url.host == bound.host && (url.port ?? 443) == (bound.port ?? 443)
    }
}
