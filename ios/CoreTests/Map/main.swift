// Проверка вспомогательного для карты на Mac (CI):
//   swiftc -swift-version 6 ios/Staya/Map/MapMath.swift ios/CoreTests/Map/main.swift -o map && ./map
import Foundation

func check(_ ok: Bool, _ what: String) {
    if !ok {
        print("FAIL: \(what)")
        exit(1)
    }
}

func distanceM(_ lat1: Double, _ lon1: Double, _ lat2: Double, _ lon2: Double) -> Double {
    let r = 6_371_008.8
    let p1 = lat1 * .pi / 180, p2 = lat2 * .pi / 180
    let dp = p2 - p1, dl = (lon2 - lon1) * .pi / 180
    let a = sin(dp / 2) * sin(dp / 2) + cos(p1) * cos(p2) * sin(dl / 2) * sin(dl / 2)
    return 2 * r * asin(sqrt(a))
}

// Круг точности замкнут и лежит на радиусе.
let ring = MapMath.accuracyRing(latE7: 557_558_000, lonE7: 376_173_000, radiusM: 250)
check(ring.count == 49, "ring size")
check(ring.first! == ring.last!, "ring closed")
for p in ring {
    check(abs(distanceM(55.7558, 37.6173, p.lat, p.lon) - 250) < 0.5, "ring radius")
}

// Подписи времени (часы друга могут спешить).
let now: Int64 = 1_700_000_000
check(MapMath.ageLabel(now: now, timestamp: now - 5) == "только что", "now")
check(MapMath.ageLabel(now: now, timestamp: now + 120) == "только что", "future")
check(MapMath.ageLabel(now: now, timestamp: now - 330) == "5 мин назад", "minutes")
check(MapMath.ageLabel(now: now, timestamp: now - 3 * 3600 - 59) == "3 ч назад", "hours")
check(MapMath.ageLabel(now: now, timestamp: now - 2 * 86_400 - 1) == "2 дн назад", "days")

// Границы из TileJSON сервера.
let tileJson = #"{"bounds":[35.1,54.2,40.25,57],"center":[37.675,55.6,0]}"#
let b = MapMath.tileJsonBounds(Data(tileJson.utf8))
check(b != nil && b!.west == 35.1 && b!.south == 54.2 && b!.east == 40.25 && b!.north == 57, "bounds")
check(MapMath.tileJsonBounds(Data(#"{"center":[1,2,3]}"#.utf8)) == nil, "no bounds")
check(MapMath.tileJsonBounds(Data(#"{"bounds":[1,2,3]}"#.utf8)) == nil, "short bounds")
check(MapMath.tileJsonBounds(Data(#"{"bounds":[40,54,35,57]}"#.utf8)) == nil, "inverted bounds")
check(MapMath.tileJsonBounds(Data(#"{"bounds":[1,2,"x",4]}"#.utf8)) == nil, "string in bounds")
check(MapMath.tileJsonBounds(Data(#"{"bounds":[1,2,true,4]}"#.utf8)) == nil, "bool in bounds")

// Только https к привязанному серверу.
let host = "staya.example"
check(MapMath.allowed(URL(string: "https://staya.example/tiles/region/1/2/3.mvt"), host: host), "bound host")
check(!MapMath.allowed(URL(string: "http://staya.example/map/style-light.json"), host: host), "http")
check(!MapMath.allowed(URL(string: "https://demotiles.maplibre.org/style.json"), host: host), "other host")
check(!MapMath.allowed(URL(string: "https://staya.example:8443/x"), host: host), "other port")
check(!MapMath.allowed(URL(string: "https://evil.staya.example/x"), host: host), "subdomain")
check(MapMath.allowed(URL(string: "https://staya.example:8443/x"), host: "staya.example:8443"), "explicit port")
check(!MapMath.allowed(URL(string: "https://staya.example/x"), host: nil), "no server")

print("map: ok")
