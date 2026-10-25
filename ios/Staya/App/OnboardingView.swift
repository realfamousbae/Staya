import PhotosUI
import StayaCore
import SwiftUI

/// Первый запуск (задачи 4.2, 4.9c, 5.2): откуда сервер, ник и аватар. Ключи создаются на
/// устройстве ещё при открытии ядра; здесь — регистрация на сервере. Проще всего —
/// отсканировать QR друга при встрече: в нём сервер, отпечаток его ключа и код
/// регистрации, а друг сразу становится проверенным (protocol §5.1).
struct OnboardingView: View {
    let core: StayaCore
    let accountId: String
    @State private var model = AppModel.shared
    @State private var link = ""
    /// Ссылка получена камерой приложения — только тогда QR даёт «проверено» (§5.1).
    @State private var scanned = false
    @State private var scanning = false
    @State private var advanced = false
    @State private var manualHost = ""
    @State private var inviteCode = ""
    @State private var nick = ""
    @State private var photo: PhotosPickerItem?
    @State private var avatar: Data?
    @State private var avatarError = false

    private var nickTooLong: Bool { nick.utf8.count > 64 }
    private var canCreate: Bool {
        !nick.trimmingCharacters(in: .whitespaces).isEmpty && !nickTooLong
            && !(link.isEmpty && manualHost.isEmpty) && !model.busy
    }

    var body: some View {
        DarkScreen(title: "staya") {
            Text("Показывает друзьям, где ты, — и никому больше. Координаты шифруются на телефоне, сервер их не видит.")
                .foregroundStyle(Ink.mist)

            SectionTitle("приглашение друга")
            Text("Рядом с другом — отсканируй QR из его Staya. Далеко — открой ссылку, которую он прислал, или вставь её сюда. Сервер и всё нужное для входа уже в приглашении.")
                .font(.footnote).foregroundStyle(Ink.mist)
            if QrScanner.isAvailable {
                Pill(text: "Сканировать QR друга") { scanning = true }
            }
            SoftButton(text: "Вставить из буфера") {
                if let text = UIPasteboard.general.string {
                    link = text
                    scanned = false
                }
            }
            // Правка руками — уже не то, что прочитала камера.
            DarkField(
                placeholder: "Ссылка-приглашение или ссылка на сервер",
                text: Binding(get: { link }, set: { link = $0; scanned = false }),
                keyboard: .URL,
                lines: 1...4
            )
            switch DeepLink.parse(link) {
            case .invite:
                Text(scanned
                    ? "QR друга отсканирован: после создания аккаунта вы станете друзьями — сразу проверенными. Он будет видеть, где ты."
                    : "Это приглашение: после создания аккаунта пригласивший станет твоим другом и будет видеть, где ты.")
                    .font(.footnote).foregroundStyle(Ink.mint)
            case .server:
                Text("Это ссылка на сервер — друзей добавишь потом.").font(.footnote).foregroundStyle(Ink.mint)
            case nil:
                EmptyView()
            }

            Button(advanced ? "Скрыть дополнительное" : "Дополнительно") { advanced.toggle() }
                .font(Ink.label).foregroundStyle(Ink.mint).buttonStyle(.plain).padding(.vertical, 4)
            // Поле кода — сразу, когда сервер его попросил; иначе в «Дополнительно».
            if model.needCode {
                DarkField(placeholder: "Код регистрации на сервере", text: $inviteCode, secure: true)
                Text("Код даёт владелец сервера.").font(.footnote).foregroundStyle(Ink.mist)
            }
            if advanced {
                DarkField(placeholder: "Сервер (host или host:port)", text: $manualHost, keyboard: .URL)
                if !model.needCode {
                    DarkField(placeholder: "Код регистрации на сервере", text: $inviteCode, secure: true)
                }
                Text("Код нужен только для закрытого сервера, если его нет в ссылке. Без отпечатка ключа сервер запоминается при первом подключении.")
                    .font(.footnote).foregroundStyle(Ink.mist)
            }

            SectionTitle("профиль")
            DarkField(placeholder: "Ник", text: $nick)
            if nickTooLong {
                Text("Слишком длинный ник").font(.footnote).foregroundStyle(Ink.danger)
            }
            HStack(spacing: 12) {
                if let avatar, let image = UIImage(data: avatar) {
                    Image(uiImage: image).resizable().frame(width: 56, height: 56).clipShape(Circle())
                }
                PhotosPicker(selection: $photo, matching: .images) {
                    Text(avatar == nil ? "Выбрать аватар" : "Сменить аватар")
                        .font(Ink.label).foregroundStyle(Ink.paper)
                        .frame(maxWidth: .infinity).frame(height: 48)
                        .background(Ink.ink2, in: Capsule())
                }
                .buttonStyle(.plain)
            }
            if avatarError {
                Text("Не получилось уменьшить фото до 8 КБ — выбери другое").font(.footnote).foregroundStyle(Ink.danger)
            }

            if let error = model.error {
                Text(error).foregroundStyle(Ink.danger)
            }

            if model.busy {
                ProgressView().tint(Ink.mint).frame(maxWidth: .infinity).frame(height: 52)
            } else {
                Pill(text: "Создать аккаунт", enabled: canCreate) {
                    model.createAccount(
                        core: core, accountId: accountId, link: link, manualHost: manualHost,
                        inviteCode: inviteCode, nick: nick, avatar: avatar, scanned: scanned
                    )
                }
            }
        }
        .toolbar(.hidden, for: .navigationBar)
        // Ссылка, открытая извне до создания аккаунта, — в поле (без автоматического принятия).
        .task(id: model.pendingLink) {
            if let pending = model.pendingLink {
                link = pending.uri
                scanned = false
                model.pendingLink = nil
            }
        }
        .sheet(isPresented: $scanning) {
            DarkScreen(title: "QR-код друга", onBack: { scanning = false }) {
                QrScanner { found in
                    link = found.uri
                    scanned = true
                    scanning = false
                }
                .aspectRatio(1, contentMode: .fit)
                .clipShape(RoundedRectangle(cornerRadius: 28))
                Text("Кадры никуда не сохраняются и не отправляются.").font(.footnote).foregroundStyle(Ink.mist)
            }
            .presentationBackground(Ink.ink)
        }
        .onChange(of: photo) { _, item in
            Task {
                guard let item, let data = try? await item.loadTransferable(type: Data.self),
                      let image = UIImage(data: data) else { return }
                avatar = Avatar.encode(image)
                avatarError = avatar == nil
            }
        }
    }
}
