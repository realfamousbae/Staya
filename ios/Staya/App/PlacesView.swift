import StayaCore
import SwiftUI
import UserNotifications

struct PlacesView: View {
    let core: StayaCore
    @Environment(\.dismiss) private var dismiss
    @State private var places: [PlaceView] = []
    @State private var friends: [FriendView] = []
    @State private var friend: String?
    @State private var name = ""
    @State private var radius = "250"
    @State private var picked: (Int32, Int32)?
    @State private var busy = false
    @State private var message: String?

    var body: some View {
        DarkScreen(title: "Места", onBack: { dismiss() }) {
            Text("Места хранятся только на этом телефоне. Уведомления появятся, когда придёт свежая позиция друга.").foregroundStyle(Ink.mist)
            if let message { Text(message).foregroundStyle(Ink.mist) }
            ForEach(places, id: \.id) { place in
                ListRow(place.name, "\(friends.first { $0.accountId == place.friend }?.nick ?? "Друг") · \(place.radiusM) м")
                Button("Удалить место", role: .destructive) { remove(place.id) }.disabled(busy)
            }
            SectionTitle("Новое место")
            if friends.isEmpty { Text("Сначала добавь друга и дождись подтверждения.").foregroundStyle(Ink.mist) }
            ForEach(friends, id: \.accountId) { value in
                ListRow(value.nick ?? "Друг", friend == value.accountId ? "Выбран" : "Выбрать друга", enabled: !busy) { friend = value.accountId }
            }
            Text("Нажми и удерживай точку на карте. Затем укажи название и радиус.").foregroundStyle(Ink.mist)
            FriendsMapView(core: core, friends: [], now: Int64(Date().timeIntervalSince1970), focus: nil,
                onPickPlace: { lat, lon in picked = (lat, lon) }, pickedPlace: picked)
                .frame(height: 280).clipShape(RoundedRectangle(cornerRadius: 16))
            Text(picked == nil ? "Точка пока не выбрана" : "Точка выбрана").foregroundStyle(Ink.mist)
            TextField("Название места", text: $name).textFieldStyle(.roundedBorder).disabled(busy)
            TextField("Радиус, м (100–5000)", text: $radius).keyboardType(.numberPad).textFieldStyle(.roundedBorder).disabled(busy)
            Button("Сохранить место") { save() }.disabled(!valid || busy).buttonStyle(.borderedProminent)
        }
        .task {
            do {
                let result = try await StayaNet.shared.queue.run { (try core.listPlaces(), try core.listFriends()) }
                places = result.0
                friends = result.1.filter(\.active)
            } catch { message = "Не удалось прочитать места." }
        }
    }

    private var valid: Bool {
        guard let meters = UInt16(radius), (100...5000).contains(meters) else { return false }
        return friend != nil && picked != nil && !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 64
    }
    private func save() {
        guard let friend, let picked, let meters = UInt16(radius), valid else { return }
        busy = true
        let label = name
        Task {
            do {
                places = try await StayaNet.shared.queue.run {
                    _ = try core.addPlace(friend: friend, name: label, latE7: picked.0, lonE7: picked.1, radiusM: meters)
                    return try core.listPlaces()
                }
                name = ""
                self.picked = nil
                let granted = (try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])) ?? false
                message = granted ? "Место сохранено." : "Место сохранено. Разреши уведомления в настройках телефона, чтобы получать сообщения."
            } catch { message = "Не удалось сохранить место. Проверь название и радиус." }
            busy = false
        }
    }
    private func remove(_ id: String) {
        busy = true
        Task {
            do {
                places = try await StayaNet.shared.queue.run { try core.removePlace(place: id); return try core.listPlaces() }
                PlaceNotifications.cancel(id)
            }
            catch { message = "Не удалось удалить место." }
            busy = false
        }
    }
}
