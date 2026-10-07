import PhotosUI
import StayaCore
import SwiftUI

/// Первый запуск (задачи 4.2, 4.9c): откуда сервер, ник и аватар. Ключи создаются на
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
        Form {
            Section {
                Text("Staya показывает друзьям, где ты, — и никому больше. Координаты шифруются на телефоне, сервер их не видит.")
                    .font(.callout)
            }

            Section {
                if QrScanner.isAvailable {
                    Button("Сканировать QR друга") { scanning = true }
                }
                Button("Вставить ссылку") {
                    if let text = UIPasteboard.general.string {
                        link = text
                        scanned = false
                    }
                }
                // Правка руками — уже не то, что прочитала камера.
                TextField(
                    "Ссылка-приглашение или ссылка на сервер",
                    text: Binding(get: { link }, set: { link = $0; scanned = false }),
                    axis: .vertical
                )
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .lineLimit(1...4)
                switch DeepLink.parse(link) {
                case .invite:
                    Text(scanned
                        ? "QR друга отсканирован: после создания аккаунта вы станете друзьями — сразу проверенными. Он будет видеть, где ты."
                        : "Это приглашение: после создания аккаунта пригласивший станет твоим другом и будет видеть, где ты.")
                        .font(.footnote)
                case .server:
                    Text("Это ссылка на сервер — друзей добавишь потом.").font(.footnote)
                case nil:
                    EmptyView()
                }
            } header: {
                Text("Приглашение друга")
            } footer: {
                Text("Рядом с другом — отсканируй QR из его Staya. Далеко — открой ссылку, которую он прислал, или вставь её сюда. Сервер и всё нужное для входа уже в приглашении.")
            }

            if model.needCode {
                Section {
                    SecureField("Код регистрации на сервере", text: $inviteCode)
                } footer: {
                    Text("Код даёт владелец сервера.")
                }
            }

            Section {
                DisclosureGroup("Дополнительно") {
                    TextField("Сервер (host или host:port)", text: $manualHost)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    if !model.needCode {
                        SecureField("Код регистрации на сервере", text: $inviteCode)
                    }
                    Text("Код нужен только для закрытого сервера, если его нет в ссылке. Без отпечатка ключа сервер запоминается при первом подключении.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }

            Section("Профиль") {
                let current = avatar
                TextField("Ник", text: $nick)
                if nickTooLong {
                    Text("Слишком длинный ник").font(.footnote).foregroundStyle(.red)
                }
                PhotosPicker(selection: $photo, matching: .images) {
                    AvatarLabel(data: current)
                }
                if avatarError {
                    Text("Не получилось уменьшить фото до 8 КБ — выбери другое").font(.footnote).foregroundStyle(.red)
                }
            }

            if let error = model.error {
                Section { Text(error).foregroundStyle(.red) }
            }

            Section {
                Button {
                    model.createAccount(
                        core: core, accountId: accountId, link: link, manualHost: manualHost,
                        inviteCode: inviteCode, nick: nick, avatar: avatar, scanned: scanned
                    )
                } label: {
                    HStack {
                        Text("Создать аккаунт")
                        if model.busy { Spacer(); ProgressView() }
                    }
                }
                .disabled(!canCreate)
            }
        }
        .navigationTitle("Staya")
        // Ссылка, открытая извне до создания аккаунта, — в поле (без автоматического принятия).
        .task(id: model.pendingLink) {
            if let pending = model.pendingLink {
                link = pending.uri
                scanned = false
                model.pendingLink = nil
            }
        }
        .sheet(isPresented: $scanning) {
            NavigationStack {
                QrScanner { found in
                    link = found.uri
                    scanned = true
                    scanning = false
                }
                .ignoresSafeArea()
                .navigationTitle("QR-код друга")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Отмена") { scanning = false } } }
            }
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

private struct AvatarLabel: View {
    let data: Data?

    var body: some View {
        HStack {
            if let data, let image = UIImage(data: data) {
                Image(uiImage: image).resizable().frame(width: 44, height: 44).clipShape(Circle())
            }
            Text(data == nil ? "Выбрать аватар" : "Сменить аватар")
        }
    }
}
