import StayaCore
import SwiftUI

/// Главный экран (5.2): карта во весь экран, хром поверх неё; друзья и добавление —
/// шторки, остальное — тёмные экраны в `.sheet`.
struct HomeView: View {
    let core: StayaCore
    let nick: String
    let server: String
    let accountId: String
    @State private var model = FriendsModel.shared
    @State private var engine = LocationEngine.shared
    @State private var focus: MapFocus?
    @State private var friendsSheet = false
    @State private var addSheet = false
    @State private var now = Int64(Date().timeIntervalSince1970)
    @Environment(\.colorScheme) private var colorScheme

    private var onMap: Int { model.friends.filter { f in f.location.map { $0.kind != .hidden } ?? false }.count }
    private var waiting: Int { model.friends.filter { !$0.active }.count }
    private var frozen: Bool { engine.enabled && engine.isFrozen(core: core) }

    var body: some View {
        let dark = colorScheme == .dark
        let textColor = dark ? Ink.paper : Ink.ink
        let halo = dark ? Ink.ink : Ink.paper
        ZStack {
            FriendsMapView(core: core, friends: model.friends, now: now, focus: focus)
                .ignoresSafeArea()

            VStack(alignment: .leading, spacing: 2) {
                // Заголовок-состояние: главное, что должен видеть владелец, — виден ли он сейчас.
                Button { model.screen = .settings } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(!engine.enabled ? "позиция скрыта" : frozen ? "заморожено здесь" : "делюсь позицией")
                            .font(Ink.display)
                        Text(subtitle).font(.subheadline)
                    }
                    .foregroundStyle(textColor)
                    .shadow(color: halo, radius: 6)
                    .multilineTextAlignment(.leading)
                }
                .buttonStyle(.plain)
                if !engine.enabled {
                    Pill(text: "Делиться позицией") { model.screen = .settings }
                        .frame(width: 220)
                        .padding(.top, 12)
                }
                if let message = model.message {
                    MessageCard(message) { model.message = nil }.padding(.top, 12)
                }
            }
            .padding(.leading, 20)
            .padding(.trailing, 84)
            .padding(.top, 16)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)

            RoundButton(system: "gearshape.fill", label: "Настройки", size: 48) { model.screen = .settings }
                .padding(.trailing, 20)
                .padding(.top, 16)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)

            HStack(spacing: 16) {
                RoundButton(system: "person.2.fill", label: "Друзья") { friendsSheet = true }
                RoundButton(system: "location.fill", label: "Показать всех") { focus = MapFocus(friendId: nil) }
                RoundButton(system: "plus", label: "Добавить друга") { addSheet = true }
            }
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        }
        .toolbar(.hidden, for: .navigationBar)
        // Подписи давности устаревают: обновляем раз в минуту, пока экран открыт.
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(60))
                now = Int64(Date().timeIntervalSince1970)
            }
        }
        .sheet(isPresented: $friendsSheet) {
            FriendsSheet(now: now) { friend in
                friendsSheet = false
                focus = MapFocus(friendId: friend.accountId)
            } onMore: { friend in
                friendsSheet = false
                model.showFriend(friend)
            }
        }
        .sheet(isPresented: $addSheet) {
            AddSheet { addSheet = false }
        }
        .sheet(item: $model.screen) { screen in
            ScreenSheet(core: core, nick: nick, server: server, screen: screen)
        }
    }

    private var subtitle: String {
        switch (model.friends.isEmpty, onMap, waiting) {
        case (true, _, _): "Друзей пока нет"
        case (_, 0, let w) where w > 0: "Ждём ответа: \(w)"
        case (_, 0, _): "Никого на карте"
        case (_, let n, let w) where w > 0: "На карте: \(n), ждём ответа: \(w)"
        case (_, let n, _): "На карте: \(n)"
        }
    }
}

/// Сообщение модели: карточка с «Понятно».
struct MessageCard: View {
    let text: String
    let dismiss: () -> Void
    init(_ text: String, dismiss: @escaping () -> Void) {
        self.text = text
        self.dismiss = dismiss
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(text).font(.subheadline).foregroundStyle(Ink.paper)
            Button("Понятно", action: dismiss).font(Ink.label).foregroundStyle(Ink.mint).buttonStyle(.plain)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Ink.ink2, in: RoundedRectangle(cornerRadius: 18))
    }
}

private struct FriendsSheet: View {
    let now: Int64
    let onShow: (FriendView) -> Void
    let onMore: (FriendView) -> Void
    @State private var model = FriendsModel.shared

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                Text("друзья").font(Ink.title).foregroundStyle(Ink.paper).padding(.bottom, 8)
                if model.friends.isEmpty {
                    Text("Пока никого. Покажи QR-код другу рядом или отправь ссылку.").foregroundStyle(Ink.mist)
                    Pill(text: "Показать мой QR-код") { model.invite(.qr) }.padding(.top, 12)
                }
                ForEach(model.friends, id: \.accountId) { friend in
                    FriendRow(friend: friend, now: now, onShow: { onShow(friend) }, onMore: { onMore(friend) })
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 24)
        }
        .presentationBackground(Ink.ink2)
        .presentationDragIndicator(.visible)
        .presentationDetents([.medium, .large])
        .preferredColorScheme(.dark)
    }
}

private struct FriendRow: View {
    let friend: FriendView
    let now: Int64
    let onShow: () -> Void
    let onMore: () -> Void

    var body: some View {
        let visible = friend.location.map { $0.kind != .hidden } ?? false
        let fresh = friend.location.map { $0.kind != .hidden && now - $0.timestamp < 10 * 60 } ?? false
        let status = friend.active ? (locationStatus(friend, now: now) ?? "пока нет позиции") : "ждём ответа"
        let unverified = friend.active && !friend.verified
        ListRow(
            title: friend.nick ?? "Без имени",
            subtitle: unverified ? "\(status) · не проверен — сверьте код безопасности" : status,
            subtitleColor: unverified ? Ink.danger : fresh ? Ink.fresh : Ink.mist,
            enabled: visible,
            action: visible ? onShow : nil,
            leading: { AvatarCircle(avatar: friend.avatar, nick: friend.nick) },
            trailing: {
                Button("Ещё", action: onMore).font(Ink.label).foregroundStyle(Ink.mint).buttonStyle(.plain)
                    .padding(.horizontal, 10).padding(.vertical, 8)
            }
        )
    }
}

private struct AddSheet: View {
    let close: () -> Void
    @State private var model = FriendsModel.shared

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                Text("добавить друга").font(Ink.title).foregroundStyle(Ink.paper).padding(.bottom, 8)
                Pill(text: "Показать мой QR-код") { close(); model.invite(.qr) }
                Spacer().frame(height: 8)
                ListRow("Отправить ссылку", "Действует сутки, откроется из мессенджера") { close(); model.invite(.link) }
                ListRow("Сканировать QR-код друга", "Рядом с другом — сразу проверенный") { close(); model.screen = .scan }
                ListRow("Вставить ссылку друга", "Из буфера обмена") {
                    close()
                    if let link = DeepLink.parse(UIPasteboard.general.string) {
                        model.open(link)
                    } else {
                        model.message = "В буфере нет ссылки Staya. Скопируй приглашение целиком."
                    }
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 24)
        }
        .presentationBackground(Ink.ink2)
        .presentationDragIndicator(.visible)
        .presentationDetents([.medium])
        .preferredColorScheme(.dark)
    }
}

/// Экраны из `FriendsModel.Screen`: тёмные, с круглой кнопкой «назад».
private struct ScreenSheet: View {
    let core: StayaCore
    let nick: String
    let server: String
    let screen: FriendsModel.Screen
    @State private var model = FriendsModel.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        content
            .presentationBackground(Ink.ink)
            .presentationDragIndicator(.visible)
    }

    @ViewBuilder
    private var content: some View {
        switch screen {
        case .settings:
            DarkScreen(title: "настройки", onBack: { dismiss() }) {
                if let message = model.message { MessageCard(message) { model.message = nil } }
                SharingSection(core: core)
                SectionTitle("аккаунт")
                ListRow("Ник", nick)
                ListRow("Сервер", server)
                Text("Координаты шифруются на телефоне — сервер видит только зашифрованные пакеты.")
                    .font(.footnote).foregroundStyle(Ink.mist)
            }
        case .showQr(let uri, let expiresAt):
            ShowQrView(uri: uri, expiresAt: expiresAt, onBack: { dismiss() })
        case .shareLink(let uri):
            DarkScreen(title: "ссылка-приглашение", onBack: { dismiss() }) {
                Text("Ссылка действует 24 часа, сработает один раз и откроется прямо из мессенджера. В ней адрес сервера и код регистрации — другу ничего вводить не нужно.")
                    .foregroundStyle(Ink.paper)
                Text("Если друг ответит, а у тебя выключено «Делиться позицией», открой Staya в течение суток. Добавленный по ссылке друг будет «не проверен», пока вы не сверите код безопасности.")
                    .font(.footnote).foregroundStyle(Ink.mist)
                // https-вид (protocol §5.4): staya:// мессенджеры не делают нажимаемым.
                ShareLink(item: webLink(uri: uri) ?? uri) {
                    Text("Отправить").font(Ink.label).foregroundStyle(Ink.ink)
                        .frame(maxWidth: .infinity).frame(height: 52)
                        .background(Ink.accent).clipShape(Capsule())
                }
            }
        case .scan:
            DarkScreen(title: "QR-код друга", onBack: { dismiss() }) {
                if QrScanner.isAvailable {
                    QrScanner { model.open($0, scanned: true) }
                        .aspectRatio(1, contentMode: .fit)
                        .clipShape(RoundedRectangle(cornerRadius: 28))
                    Text("Наведи камеру на QR-код в Staya друга. Кадры никуда не сохраняются и не отправляются.")
                        .foregroundStyle(Ink.mist)
                } else {
                    Text("Сканер на этом устройстве недоступен. Попроси друга отправить ссылку и вставь её.")
                        .foregroundStyle(Ink.mist)
                }
            }
        case .confirm(let uri, let info, let scanned):
            DarkScreen(title: "добавить друга?", onBack: { dismiss() }) {
                Text("Этот человек будет видеть, где ты, пока ты не скроешь позицию или не удалишь его.").foregroundStyle(Ink.paper)
                ListRow("Сервер", info.server)
                // Способ — как приглашение получено здесь, а не что написано в ссылке (§5.1).
                Text(scanned && info.method == .qr
                     ? "Код отсканирован при встрече — друг будет проверенным."
                     : "Приглашение получено ссылкой — сверьте потом код безопасности.")
                    .font(.footnote).foregroundStyle(Ink.mist)
                if let message = model.message { MessageCard(message) { model.message = nil } }
                Pill(text: "Добавить", enabled: !model.busy) { model.accept(uri, scanned: scanned) }
            }
        case .confirmServerCode(let uri, let host):
            DarkScreen(title: "обновить код?", onBack: { dismiss() }) {
                Text("В ссылке новый код регистрации для сервера \(host). Он уйдёт в твои приглашения, чтобы друзьям не нужно было его вводить.")
                    .foregroundStyle(Ink.paper)
                Text("Обновляй, только если ссылку прислал владелец сервера: с чужим кодом друзья не смогут зарегистрироваться.")
                    .font(.footnote).foregroundStyle(Ink.mist)
                Pill(text: "Обновить") { model.updateCode(uri) }
                SoftButton(text: "Отмена") { dismiss() }
            }
        case .friend(let friend, let code):
            FriendCard(initial: friend, code: code, onBack: { dismiss() })
        }
    }
}

/// Карточка друга (4.6): что он видит, код безопасности, удаление.
private struct FriendCard: View {
    let initial: FriendView
    let code: String
    let onBack: () -> Void
    @State private var model = FriendsModel.shared
    @State private var confirmRemove = false

    /// Свежая версия из списка: точность и статус меняются, пока карточка открыта.
    private var friend: FriendView {
        model.friends.first { $0.accountId == initial.accountId } ?? initial
    }

    var body: some View {
        DarkScreen(title: friend.nick ?? "друг", onBack: onBack) {
            HStack(spacing: 16) {
                AvatarCircle(avatar: friend.avatar, nick: friend.nick, size: 64)
                VStack(alignment: .leading, spacing: 2) {
                    Text(!friend.active ? "ждём ответа" : friend.verified ? "проверен" : "не проверен")
                        .font(Ink.heading)
                        .foregroundStyle(friend.active && !friend.verified ? Ink.danger : Ink.paper)
                    if let status = locationStatus(friend, now: Int64(Date().timeIntervalSince1970)) {
                        Text(status).font(.footnote).foregroundStyle(Ink.mist)
                    }
                }
            }
            if let message = model.message { MessageCard(message) { model.message = nil } }

            if friend.active {
                SectionTitle("что видит этот друг")
                ForEach([(Precision.exact, "Точную позицию"), (.approx, "Примерно (район ~1 км)"), (.hidden, "Ничего — «скрыл(а) позицию»")], id: \.1) { precision, title in
                    Button { model.setPrecision(friend, precision) } label: {
                        HStack(spacing: 12) {
                            Image(systemName: friend.precision == precision ? "largecircle.fill.circle" : "circle")
                                .foregroundStyle(friend.precision == precision ? Ink.mint : Ink.mist)
                            Text(title).foregroundStyle(Ink.paper)
                            Spacer()
                        }
                        .padding(.vertical, 6)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }

            SectionTitle("код безопасности")
            Text(code.chunks(of: 5).joined(separator: " "))
                .font(.system(size: 17, weight: .medium, design: .monospaced))
                .foregroundStyle(Ink.paper)
            Text("Сравните код при встрече или голосом. Совпадает — значит, между вами никого нет. Если код другой — не нажимай «Совпадает» и удали этого друга.")
                .font(.footnote).foregroundStyle(Ink.mist)
            if friend.verified {
                Text("Уже проверен.").foregroundStyle(Ink.fresh)
            } else if friend.active {
                Pill(text: "Совпадает") { model.markVerified(friend) }
            }

            Spacer().frame(height: 8)
            SoftButton(text: "Удалить друга", color: Ink.danger) { confirmRemove = true }
        }
        .confirmationDialog("Удалить \(friend.nick ?? "друга")?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Удалить", role: .destructive) { model.remove(friend) }
        } message: {
            Text("Он сразу перестанет видеть твою позицию, а ты — его. Чтобы снова дружить, придётся добавить друг друга заново.")
        }
    }
}

private extension String {
    func chunks(of n: Int) -> [String] {
        stride(from: 0, to: count, by: n).map { i in
            let start = index(startIndex, offsetBy: i)
            let end = index(start, offsetBy: n, limitedBy: endIndex) ?? endIndex
            return String(self[start..<end])
        }
    }
}

private struct ShowQrView: View {
    let uri: String
    let expiresAt: Date
    let onBack: () -> Void
    @State private var model = FriendsModel.shared

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let left = Int(expiresAt.timeIntervalSince(context.date))
            DarkScreen(title: "мой QR-код", onBack: onBack) {
                Text("Покажи код другу рядом. Добавленный по QR при встрече друг сразу считается проверенным.")
                    .foregroundStyle(Ink.mist)
                ZStack {
                    RoundedRectangle(cornerRadius: 28).fill(Ink.paper)
                    if left > 0, let image = Qr.image(uri) {
                        Image(uiImage: image)
                            .interpolation(.none)
                            .resizable()
                            .scaledToFit()
                            .padding(20)
                    } else {
                        Text("Код истёк").font(Ink.title).foregroundStyle(Ink.ink).padding(.vertical, 60)
                    }
                }
                .aspectRatio(1, contentMode: .fit)
                if left > 0 {
                    Text("Действует ещё \(left / 60):\(String(format: "%02d", left % 60)). Не закрывай приложение, пока друг не отсканирует.")
                        .foregroundStyle(Ink.mist)
                }
                Pill(text: "Новый код") { model.invite(.qr) }
            }
        }
    }
}
