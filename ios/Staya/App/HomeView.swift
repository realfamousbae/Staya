import StayaCore
import SwiftUI

/// Главный экран (пока заготовка): карта и друзья — задачи 4.3–4.4.
struct HomeView: View {
    let nick: String
    let server: String
    let accountId: String

    var body: some View {
        List {
            Section {
                LabeledContent("Ник", value: nick)
                LabeledContent("Сервер", value: server)
                LabeledContent("Аккаунт", value: String(accountId.prefix(8)) + "…")
            }
            Section {
                Text("Карта и друзья появятся в следующих версиях.").foregroundStyle(.secondary)
            }
        }
        .navigationTitle(nick)
    }
}
