import PhotosUI
import StayaCore
import SwiftUI

/// Первый запуск (задача 4.2): откуда сервер, ник и аватар. Ключи создаются на
/// устройстве ещё при открытии ядра; здесь — регистрация на сервере.
struct OnboardingView: View {
    let core: StayaCore
    let accountId: String
    @State private var model = AppModel.shared
    @State private var link = ""
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
                TextField("staya://…", text: $link, axis: .vertical)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .lineLimit(1...4)
                Button("Вставить") { link = UIPasteboard.general.string ?? link }
                if case .invite = DeepLink.parse(link) {
                    Text("Это приглашение: после создания аккаунта пригласивший станет твоим другом и будет видеть, где ты.")
                        .font(.footnote)
                }
            } header: {
                Text("Приглашение или ссылка на сервер")
            } footer: {
                Text("Приглашение присылает друг. Ссылку на сервер — его владелец.")
            }

            Section {
                DisclosureGroup("Дополнительно") {
                    TextField("Сервер (host или host:port)", text: $manualHost)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    SecureField("Код приглашения на регистрацию", text: $inviteCode)
                    Text("Код нужен только для закрытого сервера. Без отпечатка ключа сервер запоминается при первом подключении.")
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
                        inviteCode: inviteCode, nick: nick, avatar: avatar
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
                model.pendingLink = nil
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
