import Foundation
import StayaCore
import UserNotifications

enum PlaceNotifications {
    static func cancel(_ place: String) {
        let center = UNUserNotificationCenter.current()
        center.removeDeliveredNotifications(withIdentifiers: ["place-\(place)"])
        center.removePendingNotificationRequests(withIdentifiers: ["place-\(place)"])
    }
    static func cancelFriend(_ friend: String) async {
        let center = UNUserNotificationCenter.current()
        let notes = await center.deliveredNotifications()
        center.removeDeliveredNotifications(withIdentifiers: notes.filter { $0.request.content.userInfo["friend"] as? String == friend }.map { $0.request.identifier })
    }
    static func post(core: StayaCore, events: [CoreEvent]) async {
        let center = UNUserNotificationCenter.current()
        let removed = events.compactMap { event -> String? in
            if case .friendRemoved(let friend) = event { return friend }
            return nil
        }
        if !removed.isEmpty {
            let notes = await center.deliveredNotifications()
            center.removeDeliveredNotifications(withIdentifiers: notes.filter { note in
                guard let friend = note.request.content.userInfo["friend"] as? String else { return false }
                return removed.contains(friend)
            }.map { $0.request.identifier })
        }
        let transitions = events.compactMap { event -> (String, String, String, Bool)? in
            if case .placeTransition(let friend, let place, let name, let entered) = event { return (friend, place, name, entered) }
            return nil
        }
        guard !transitions.isEmpty else { return }
        let settings = await center.notificationSettings()
        guard settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional else { return }
        let protected = UserDefaults.standard.bool(forKey: "appLockEnabled")
        let friends = (try? core.listFriends()) ?? []
        for (friend, place, name, entered) in transitions {
            let content = UNMutableNotificationContent()
            content.title = "Staya — места"
            let nick = friends.first { $0.accountId == friend }?.nick ?? "Друг"
            content.body = protected ? "Изменилось присутствие друга. Открой Staya." : "\(nick) \(entered ? "пришёл" : "ушёл"): \(name)"
            content.userInfo = ["friend": friend]
            content.sound = .default
            let identifier = "place-\(place)"
            center.removeDeliveredNotifications(withIdentifiers: [identifier])
            try? await center.add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil))
        }
    }
}
