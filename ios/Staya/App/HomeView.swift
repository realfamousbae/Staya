import StayaCore
import SwiftUI

/// Главный экран: друзья и добавление (4.3). Карта — 4.4.
struct HomeView: View {
    let nick: String
    let server: String
    let accountId: String
    @State private var model = FriendsModel.shared

    var body: some View {
        List {
            if let message = model.message {
                Section {
                    Text(message)
                    Button("Понятно") { model.message = nil }
                }
            }
            Section("Друзья") {
                if model.friends.isEmpty {
                    Text("Пока никого. Покажи QR-код другу рядом или отправь ссылку.").foregroundStyle(.secondary)
                }
                ForEach(model.friends, id: \.accountId) { friend in
                    Button { model.showSafety(friend) } label: { FriendRow(friend: friend) }
                        .foregroundStyle(.primary)
                }
            }
            Section("Добавить друга") {
                Button("Показать мой QR-код") { model.invite(.qr) }
                Button("Отправить ссылку") { model.invite(.link) }
                Button("Сканировать QR-код друга") { model.screen = .scan }
                Button("Вставить ссылку друга") {
                    if let link = DeepLink.parse(UIPasteboard.general.string) {
                        model.open(link)
                    } else {
                        model.message = "В буфере нет ссылки staya://. Скопируй приглашение целиком."
                    }
                }
            }
            Section {
                LabeledContent("Сервер", value: server)
            }
        }
        .navigationTitle(nick)
        .refreshable { model.reload() }
        .sheet(item: $model.screen) { screen in
            NavigationStack { FriendsSheet(screen: screen) }
        }
    }
}

private struct FriendRow: View {
    let friend: FriendView

    var body: some View {
        HStack {
            if let data = friend.avatar, !data.isEmpty, let image = UIImage(data: data) {
                Image(uiImage: image).resizable().frame(width: 40, height: 40).clipShape(Circle())
            }
            VStack(alignment: .leading) {
                Text(friend.nick ?? "Без имени")
                Text(!friend.active ? "ждём ответа" : friend.verified ? "проверен" : "не проверен — сверьте код безопасности")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct FriendsSheet: View {
    let screen: FriendsModel.Screen
    @State private var model = FriendsModel.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        content
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Закрыть") { dismiss() } }
            }
    }

    @ViewBuilder
    private var content: some View {
        switch screen {
        case .showQr(let uri, let expiresAt):
            ShowQrView(uri: uri, expiresAt: expiresAt)
        case .shareLink(let uri):
            Form {
                Text("Ссылка действует 24 часа. Открой Staya в течение суток, чтобы принять ответ друга. Добавленный по ссылке друг будет «не проверен», пока вы не сверите код безопасности.")
                ShareLink("Отправить", item: uri)
            }
            .navigationTitle("Ссылка-приглашение")
        case .scan:
            Group {
                if QrScanner.isAvailable {
                    QrScanner { model.open($0) }.ignoresSafeArea()
                } else {
                    ContentUnavailableView(
                        "Сканер недоступен",
                        systemImage: "qrcode.viewfinder",
                        description: Text("Попроси друга отправить ссылку и вставь её.")
                    )
                }
            }
            .navigationTitle("QR-код друга")
        case .confirm(let uri, let info):
            Form {
                Text("Этот человек будет видеть, где ты, пока ты не скроешь позицию или не удалишь его.")
                LabeledContent("Сервер", value: info.server)
                Text(info.method == .qr
                     ? "Код отсканирован при встрече — друг будет проверенным."
                     : "Приглашение по ссылке — сверьте потом код безопасности.")
                    .font(.footnote)
                if let message = model.message { Text(message).foregroundStyle(.red) }
                Button("Добавить") { model.accept(uri) }.disabled(model.busy)
            }
            .navigationTitle("Добавить друга?")
        case .safety(let friend, let code):
            Form {
                Text(code).font(.title3.monospaced())
                Text("Сравните код при встрече или голосом. Совпадает — значит, между вами никого нет. Если код другой — не нажимай «Совпадает» и удали этого друга.")
                    .font(.footnote)
                if friend.verified {
                    Text("Уже проверен.").foregroundStyle(.green)
                } else {
                    Button("Совпадает") { model.markVerified(friend) }
                }
            }
            .navigationTitle(friend.nick ?? "Код безопасности")
        }
    }
}

private struct ShowQrView: View {
    let uri: String
    let expiresAt: Date
    @State private var model = FriendsModel.shared

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let left = Int(expiresAt.timeIntervalSince(context.date))
            VStack(spacing: 16) {
                if left > 0, let image = Qr.image(uri) {
                    Image(uiImage: image)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .padding()
                    Text("Действует ещё \(left / 60):\(String(format: "%02d", left % 60)). Не закрывай приложение, пока друг не отсканирует.")
                        .multilineTextAlignment(.center)
                } else {
                    Text("Код истёк.")
                }
                Button("Новый код") { model.invite(.qr) }
                Text("Добавленный по QR при встрече друг сразу считается проверенным.")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            .padding()
        }
        .navigationTitle("Покажи код другу")
    }
}
