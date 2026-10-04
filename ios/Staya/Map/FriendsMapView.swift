import MapLibre
import StayaCore
import SwiftUI
import UIKit

/// Куда сдвинуть карту: друг из списка (по нажатию).
struct MapFocus: Equatable {
    let friendId: String
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

/// Карта друзей (задача 4.4): стиль и тайлы со своего сервера, маркер — аватар и
/// «ник · N мин назад», круг точности. Скрывшие позицию на карте не показываются.
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
            c.show(friendId: focus.friendId)
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

        private static let friendsSource = "staya-friends"
        private static let accuracySource = "staya-accuracy"

        @objc(mapView:didFinishLoadingStyle:)
        func mapView(_ mapView: MLNMapView, didFinishLoading style: MLNStyle) {
            let accuracy = MLNShapeSource(identifier: Self.accuracySource, shape: nil, options: nil)
            let points = MLNShapeSource(identifier: Self.friendsSource, shape: nil, options: nil)
            style.addSource(accuracy)
            style.addSource(points)
            let blue = UIColor(red: 0.18, green: 0.44, blue: 0.93, alpha: 1)

            let fill = MLNFillStyleLayer(identifier: "staya-accuracy-fill", source: accuracy)
            fill.fillColor = NSExpression(forConstantValue: blue)
            fill.fillOpacity = NSExpression(forConstantValue: 0.12)
            style.addLayer(fill)

            let line = MLNLineStyleLayer(identifier: "staya-accuracy-line", source: accuracy)
            line.lineColor = NSExpression(forConstantValue: blue)
            line.lineOpacity = NSExpression(forConstantValue: 0.5)
            line.lineWidth = NSExpression(forConstantValue: 1)
            style.addLayer(line)

            let symbols = MLNSymbolStyleLayer(identifier: "staya-friends", source: points)
            symbols.iconImageName = NSExpression(forKeyPath: "icon")
            symbols.iconAllowsOverlap = NSExpression(forConstantValue: true)
            symbols.iconIgnoresPlacement = NSExpression(forConstantValue: true)
            symbols.text = NSExpression(forKeyPath: "label")
            // Шрифты — только те, что раздаёт сервер (deploy/map/update-map.sh).
            symbols.textFontNames = NSExpression(forConstantValue: ["Noto Sans Medium"])
            symbols.textFontSize = NSExpression(forConstantValue: 12)
            symbols.textAnchor = NSExpression(forConstantValue: "top")
            symbols.textOffset = NSExpression(forConstantValue: NSValue(cgVector: CGVector(dx: 0, dy: 1.9)))
            symbols.textAllowsOverlap = NSExpression(forConstantValue: true)
            symbols.textColor = NSExpression(forConstantValue: dark ? UIColor(white: 0.95, alpha: 1) : UIColor(white: 0.1, alpha: 1))
            symbols.textHaloColor = NSExpression(forConstantValue: dark ? UIColor(white: 0.1, alpha: 1) : UIColor.white)
            symbols.textHaloWidth = NSExpression(forConstantValue: 1.5)
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
            for (f, loc) in visible {
                // Имя картинки меняется вместе с аватаром и ником — не старая из кэша стиля.
                let icon = "avatar-\(f.accountId)-\(f.avatar?.hashValue ?? 0)-\(f.nick?.hashValue ?? 0)"
                if style.image(forName: icon) == nil {
                    style.setImage(Self.markerImage(f), forName: icon)
                }
                let point = MLNPointFeature()
                point.coordinate = CLLocationCoordinate2D(latitude: Double(loc.latE7) / 1e7, longitude: Double(loc.lonE7) / 1e7)
                point.attributes = ["icon": icon, "label": (f.nick ?? "Без имени") + "\n" + (locationStatus(f, now: now) ?? "")]
                markers.append(point)
                if loc.accuracyM > 0 {
                    var ring = MapMath.accuracyRing(latE7: loc.latE7, lonE7: loc.lonE7, radiusM: Double(loc.accuracyM))
                        .map { CLLocationCoordinate2D(latitude: $0.lat, longitude: $0.lon) }
                    circles.append(MLNPolygonFeature(coordinates: &ring, count: UInt(ring.count)))
                }
            }
            points.shape = MLNShapeCollectionFeature(shapes: markers)
            accuracy.shape = MLNShapeCollectionFeature(shapes: circles)
            fitCamera()
        }

        /// Первый показ: все друзья; если их нет — регион карты сервера.
        /// Список друзей приходит после карты: сначала регион, потом — друзья.
        private func fitCamera() {
            guard let mapView else { return }
            let coords = visible.map {
                CLLocationCoordinate2D(latitude: Double($0.1.latE7) / 1e7, longitude: Double($0.1.lonE7) / 1e7)
            }
            if !coords.isEmpty, fitted < 2 {
                fitted = 2
                if coords.count == 1 {
                    mapView.setCenter(coords[0], zoomLevel: 14, animated: false)
                } else {
                    var c = coords
                    mapView.setVisibleCoordinates(&c, count: UInt(c.count), edgePadding: UIEdgeInsets(top: 80, left: 60, bottom: 80, right: 60), animated: false)
                }
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

        func show(friendId: String) {
            guard let loc = visible.first(where: { $0.0.accountId == friendId })?.1 else { return }
            let center = CLLocationCoordinate2D(latitude: Double(loc.latE7) / 1e7, longitude: Double(loc.lonE7) / 1e7)
            mapView?.setCenter(center, zoomLevel: 15, animated: true)
        }

        /// Круглый аватар с обводкой; без аватара — первая буква ника.
        private static func markerImage(_ friend: FriendView) -> UIImage {
            let size = CGSize(width: 44, height: 44)
            return UIGraphicsImageRenderer(size: size).image { _ in
                UIColor.white.setFill()
                UIBezierPath(ovalIn: CGRect(origin: .zero, size: size)).fill()
                let inner = CGRect(x: 3, y: 3, width: 38, height: 38)
                if let data = friend.avatar, !data.isEmpty, let avatar = UIImage(data: data) {
                    UIBezierPath(ovalIn: inner).addClip()
                    avatar.draw(in: inner)
                } else {
                    UIColor(red: 0.18, green: 0.44, blue: 0.93, alpha: 1).setFill()
                    UIBezierPath(ovalIn: inner).fill()
                    let letter = friend.nick?.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() } ?? "?"
                    let attrs: [NSAttributedString.Key: Any] = [
                        .font: UIFont.systemFont(ofSize: 20, weight: .semibold),
                        .foregroundColor: UIColor.white,
                    ]
                    let text = NSAttributedString(string: letter, attributes: attrs)
                    let s = text.size()
                    text.draw(at: CGPoint(x: (size.width - s.width) / 2, y: (size.height - s.height) / 2))
                }
            }
        }
    }
}
