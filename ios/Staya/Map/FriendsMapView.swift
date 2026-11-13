import MapLibre
import StayaCore
import SwiftUI
import UIKit

/// Куда сдвинуть карту: друг из списка (по нажатию) или все друзья (`friendId == nil`).
struct MapFocus: Equatable {
    let friendId: String?
    let nonce = UUID()
}

/// Подпись маркера и строка в списке: где друг и насколько свежо.
func locationStatus(_ friend: FriendView, now: Int64) -> String? {
    guard let loc = friend.location else { return nil }
    let age = MapMath.ageLabel(now: now, timestamp: loc.timestamp)
    switch loc.kind {
    case .exact: return age
    case .approx: return "примерно · \(age)"
    case .frozen: return "позиция заморожена · \(age)"
    case .hidden: return "скрыл(а) позицию"
    }
}

/// Карта друзей (задачи 4.4, 5.2): стиль и тайлы со своего сервера; маркер — аватар в
/// белом кольце с бейджем давности под ним («5 мин», зелёная точка — свежая), круг
/// точности. Скрывшие позицию на карте не показываются. Кнопки — у экрана поверх карты.
struct FriendsMapView: UIViewRepresentable {
    let core: StayaCore
    let friends: [FriendView]
    let now: Int64
    let focus: MapFocus?
    @Environment(\.colorScheme) private var colorScheme

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> MLNMapView {
        MapNetwork.shared.install(core: core)
        let dark = colorScheme == .dark
        // Всегда со стилем своего сервера: без него MapLibre загрузил бы стиль по умолчанию из сети.
        let url = MapNetwork.shared.styleURL(dark: dark) ?? URL(string: "about:blank")!
        let view = MLNMapView(frame: .zero, styleURL: url)
        view.allowsRotating = false
        view.allowsTilting = false
        // Логотип движка не нужен; атрибуция данных карты (OpenStreetMap) обязательна —
        // кнопка «i» слева снизу, над нижними кнопками экрана.
        view.logoView.isHidden = true
        view.attributionButtonPosition = .bottomLeft
        view.attributionButtonMargins = CGPoint(x: 8, y: 96)
        view.delegate = context.coordinator
        context.coordinator.mapView = view
        context.coordinator.dark = dark
        return view
    }

    func updateUIView(_ view: MLNMapView, context: Context) {
        let c = context.coordinator
        c.friends = friends
        c.now = now
        let dark = colorScheme == .dark
        if dark != c.dark, let url = MapNetwork.shared.styleURL(dark: dark) {
            c.dark = dark
            c.style = nil
            view.styleURL = url
        }
        c.refresh()
        if let focus, focus != c.lastFocus {
            c.lastFocus = focus
            if let id = focus.friendId { c.show(friendId: id) } else { c.showAll() }
        }
    }

    @MainActor
    final class Coordinator: NSObject, @preconcurrency MLNMapViewDelegate {
        weak var mapView: MLNMapView?
        var style: MLNStyle?
        var friends: [FriendView] = []
        var now: Int64 = 0
        var dark = false
        var lastFocus: MapFocus?
        /// 0 — камера не выставлена, 1 — показан регион, 2 — показаны друзья.
        private var fitted = 0
        /// Картинки маркеров по друзьям: бейдж меняется раз в минуту, старые убираем из стиля.
        private var icons: [String: String] = [:]

        private static let friendsSource = "staya-friends"
        private static let accuracySource = "staya-accuracy"

        @objc(mapView:didFinishLoadingStyle:)
        func mapView(_ mapView: MLNMapView, didFinishLoading style: MLNStyle) {
            let accuracy = MLNShapeSource(identifier: Self.accuracySource, shape: nil, options: nil)
            let points = MLNShapeSource(identifier: Self.friendsSource, shape: nil, options: nil)
            style.addSource(accuracy)
            style.addSource(points)
            let green = UIColor(red: 0.298, green: 0.851, blue: 0.482, alpha: 1)

            let fill = MLNFillStyleLayer(identifier: "staya-accuracy-fill", source: accuracy)
            fill.fillColor = NSExpression(forConstantValue: green)
            fill.fillOpacity = NSExpression(forConstantValue: 0.12)
            style.addLayer(fill)

            let line = MLNLineStyleLayer(identifier: "staya-accuracy-line", source: accuracy)
            line.lineColor = NSExpression(forConstantValue: green)
            line.lineOpacity = NSExpression(forConstantValue: 0.5)
            line.lineWidth = NSExpression(forConstantValue: 1)
            style.addLayer(line)

            let symbols = MLNSymbolStyleLayer(identifier: "staya-friends", source: points)
            symbols.iconImageName = NSExpression(forKeyPath: "icon")
            symbols.iconAllowsOverlap = NSExpression(forConstantValue: true)
            symbols.iconIgnoresPlacement = NSExpression(forConstantValue: true)
            // Точка — центр аватара, а не центр картинки с бейджем (см. markerImage).
            symbols.iconOffset = NSExpression(forConstantValue: NSValue(cgVector: CGVector(dx: 0, dy: Self.markerShift)))
            style.addLayer(symbols)

            self.style = style
            refresh()
        }

        private var visible: [(FriendView, FriendLocation)] {
            friends.compactMap { f in
                guard let loc = f.location, loc.kind != .hidden else { return nil }
                return (f, loc)
            }
        }

        func refresh() {
            guard let style,
                  let points = style.source(withIdentifier: Self.friendsSource) as? MLNShapeSource,
                  let accuracy = style.source(withIdentifier: Self.accuracySource) as? MLNShapeSource
            else { return }
            var markers: [MLNPointFeature] = []
            var circles: [MLNPolygonFeature] = []
            var seen = Set<String>()
            for (f, loc) in visible {
                let badge = MapMath.shortAge(now: now, timestamp: loc.timestamp)
                let fresh = now - loc.timestamp < 10 * 60
                // Имя картинки меняется вместе с аватаром, ником и бейджем — не старая из кэша стиля.
                let icon = "avatar-\(f.accountId)-\(f.avatar?.hashValue ?? 0)-\(f.nick?.hashValue ?? 0)-\(badge)\(fresh)"
                if let previous = icons.updateValue(icon, forKey: f.accountId), previous != icon {
                    style.removeImage(forName: previous)
                }
                seen.insert(f.accountId)
                if style.image(forName: icon) == nil {
                    style.setImage(Self.markerImage(f, badge: badge, fresh: fresh), forName: icon)
                }
                let point = MLNPointFeature()
                point.coordinate = CLLocationCoordinate2D(latitude: Double(loc.latE7) / 1e7, longitude: Double(loc.lonE7) / 1e7)
                point.attributes = ["icon": icon]
                markers.append(point)
                if loc.accuracyM > 0 {
                    var ring = MapMath.accuracyRing(latE7: loc.latE7, lonE7: loc.lonE7, radiusM: Double(loc.accuracyM))
                        .map { CLLocationCoordinate2D(latitude: $0.lat, longitude: $0.lon) }
                    circles.append(MLNPolygonFeature(coordinates: &ring, count: UInt(ring.count)))
                }
            }
            for id in Set(icons.keys).subtracting(seen) {
                if let old = icons.removeValue(forKey: id) { style.removeImage(forName: old) }
            }
            points.shape = MLNShapeCollectionFeature(shapes: markers)
            accuracy.shape = MLNShapeCollectionFeature(shapes: circles)
            fitCamera()
        }

        /// Все видимые друзья в кадре; одного — крупно. `false` — показывать было некого.
        @discardableResult
        private func fitAll(animated: Bool) -> Bool {
            guard let mapView else { return false }
            let coords = visible.map {
                CLLocationCoordinate2D(latitude: Double($0.1.latE7) / 1e7, longitude: Double($0.1.lonE7) / 1e7)
            }
            if coords.isEmpty { return false }
            if coords.count == 1 {
                mapView.setCenter(coords[0], zoomLevel: 14, animated: animated)
            } else {
                var c = coords
                mapView.setVisibleCoordinates(&c, count: UInt(c.count), edgePadding: UIEdgeInsets(top: 120, left: 60, bottom: 120, right: 60), animated: animated)
            }
            return true
        }

        /// Первый показ: все друзья; если их нет — регион карты сервера.
        /// Список друзей приходит после карты: сначала регион, потом — друзья.
        private func fitCamera() {
            guard let mapView else { return }
            if !visible.isEmpty, fitted < 2 {
                fitted = 2
                fitAll(animated: false)
            } else if fitted == 0 {
                fitted = 1
                Task { [weak mapView] in
                    guard let b = await MapNetwork.shared.regionBounds(), let mapView, self.fitted == 1 else { return }
                    let bounds = MLNCoordinateBounds(
                        sw: CLLocationCoordinate2D(latitude: b.south, longitude: b.west),
                        ne: CLLocationCoordinate2D(latitude: b.north, longitude: b.east)
                    )
                    mapView.setVisibleCoordinateBounds(bounds, animated: false)
                }
            }
        }

        func showAll() {
            fitAll(animated: true)
        }

        func show(friendId: String) {
            guard let loc = visible.first(where: { $0.0.accountId == friendId })?.1 else { return }
            let center = CLLocationCoordinate2D(latitude: Double(loc.latE7) / 1e7, longitude: Double(loc.lonE7) / 1e7)
            mapView?.setCenter(center, zoomLevel: 15, animated: true)
        }

        private static let avatarSize: CGFloat = 48
        private static let ring: CGFloat = 3
        private static let badgeHeight: CGFloat = 20
        private static let overlap: CGFloat = 6
        /// Картинка выше аватара на высоту бейджа минус перекрытие: смещаем на половину вниз.
        static let markerShift: CGFloat = (badgeHeight - overlap) / 2

        /// Аватар в белом кольце (без аватара — первая буква ника на мятном), под ним бейдж
        /// давности: тёмная пилюля с белым текстом и зелёной точкой, если позиция свежая.
        private static func markerImage(_ friend: FriendView, badge: String, fresh: Bool) -> UIImage {
            let avatar = avatarSize
            let font = UIFont.systemFont(ofSize: 11.5, weight: .bold)
            let text = NSAttributedString(string: badge, attributes: [.font: font, .foregroundColor: UIColor.white])
            let dot: CGFloat = fresh ? 6 : 0
            let badgeWidth = text.size().width + 16 + (fresh ? dot + 4 : 0)
            let width = max(avatar, badgeWidth) + 4
            let height = avatar + badgeHeight - overlap + 4
            let size = CGSize(width: width, height: height)
            let cx = width / 2
            let top: CGFloat = 2
            return UIGraphicsImageRenderer(size: size).image { ctx in
                // Кольцо и аватар.
                ctx.cgContext.setShadow(offset: CGSize(width: 0, height: 2), blur: 4, color: UIColor.black.withAlphaComponent(0.33).cgColor)
                UIColor.white.setFill()
                UIBezierPath(ovalIn: CGRect(x: cx - avatar / 2, y: top, width: avatar, height: avatar)).fill()
                ctx.cgContext.setShadow(offset: .zero, blur: 0, color: nil)
                let inner = CGRect(x: cx - avatar / 2 + ring, y: top + ring, width: avatar - 2 * ring, height: avatar - 2 * ring)
                ctx.cgContext.saveGState()
                if let data = friend.avatar, !data.isEmpty, let picture = UIImage(data: data) {
                    UIBezierPath(ovalIn: inner).addClip()
                    picture.draw(in: inner)
                } else {
                    UIColor(red: 0.663, green: 0.941, blue: 0.78, alpha: 1).setFill()
                    UIBezierPath(ovalIn: inner).fill()
                    let letter = friend.nick?.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() } ?? "?"
                    let attrs: [NSAttributedString.Key: Any] = [
                        .font: UIFont.systemFont(ofSize: avatar * 0.42, weight: .bold),
                        .foregroundColor: UIColor(red: 0.059, green: 0.059, blue: 0.071, alpha: 1),
                    ]
                    let string = NSAttributedString(string: letter, attributes: attrs)
                    let s = string.size()
                    string.draw(at: CGPoint(x: inner.midX - s.width / 2, y: inner.midY - s.height / 2))
                }
                ctx.cgContext.restoreGState()

                // Бейдж.
                let pill = CGRect(x: cx - badgeWidth / 2, y: top + avatar - overlap, width: badgeWidth, height: badgeHeight)
                let path = UIBezierPath(roundedRect: pill, cornerRadius: badgeHeight / 2)
                UIColor(red: 0.059, green: 0.059, blue: 0.071, alpha: 1).setFill()
                path.fill()
                UIColor.white.setStroke()
                path.lineWidth = 1.5
                path.stroke()
                var textX = cx
                if fresh {
                    UIColor(red: 0.298, green: 0.851, blue: 0.482, alpha: 1).setFill()
                    UIBezierPath(ovalIn: CGRect(x: pill.minX + 8, y: pill.midY - dot / 2, width: dot, height: dot)).fill()
                    textX += (dot + 4) / 2
                }
                let ts = text.size()
                text.draw(at: CGPoint(x: textX - ts.width / 2, y: pill.midY - ts.height / 2))
            }
        }
    }
}
