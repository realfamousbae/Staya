import SwiftUI

/// Оформление (задача 5.2): карта во весь экран, чернильный хром поверх неё, тёмные
/// экраны с белыми карточками — на манер Blink, без лишнего. Одна смелость —
/// крупный строчный заголовок-состояние; всё остальное тихо.
enum Ink {
    static let ink = Color(red: 0.059, green: 0.059, blue: 0.071)
    static let ink2 = Color(red: 0.11, green: 0.11, blue: 0.133)
    static let ink3 = Color(red: 0.165, green: 0.165, blue: 0.2)
    static let paper = Color.white
    static let mist = Color(red: 0.604, green: 0.604, blue: 0.651)
    static let mint = Color(red: 0.663, green: 0.941, blue: 0.78)
    static let sky = Color(red: 0.608, green: 0.816, blue: 1)
    static let fresh = Color(red: 0.298, green: 0.851, blue: 0.482)
    static let danger = Color(red: 1, green: 0.42, blue: 0.42)
    static var accent: LinearGradient { LinearGradient(colors: [mint, sky], startPoint: .leading, endPoint: .trailing) }

    /// Заголовок экрана и состояние на карте: крупный, жирный, строчные буквы.
    static let display = Font.system(size: 34, weight: .heavy)
    static let title = Font.system(size: 24, weight: .bold)
    static let heading = Font.system(size: 17, weight: .semibold)
    static let label = Font.system(size: 16, weight: .semibold)
}

/// Круглая чёрная кнопка поверх карты.
struct RoundButton: View {
    let system: String
    let label: String
    var size: CGFloat = 56
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: system)
                .font(.system(size: size * 0.38, weight: .semibold))
                .foregroundStyle(Ink.paper)
                .frame(width: size, height: size)
                .background(Ink.ink, in: Circle())
                .shadow(color: .black.opacity(0.35), radius: 8, y: 3)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

/// Главное действие экрана: пилюля с градиентом.
struct Pill: View {
    let text: String
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(text)
                .font(Ink.label)
                .foregroundStyle(enabled ? Ink.ink : Ink.mist)
                .frame(maxWidth: .infinity)
                .frame(height: 52)
                .background {
                    if enabled { Ink.accent } else { Ink.ink3 }
                }
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }
}

/// Второстепенное действие: тихая пилюля на тёмном.
struct SoftButton: View {
    let text: String
    var color: Color = Ink.paper
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(text)
                .font(Ink.label)
                .foregroundStyle(color)
                .frame(maxWidth: .infinity)
                .frame(height: 48)
                .background(Ink.ink2, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

/// Строка списка: заголовок, подпись, стрелка; разделяет только отступ.
struct ListRow<Leading: View, Trailing: View>: View {
    let title: String
    var subtitle: String?
    var subtitleColor: Color = Ink.mist
    var enabled = true
    var action: (() -> Void)?
    @ViewBuilder var leading: Leading
    @ViewBuilder var trailing: Trailing

    var body: some View {
        let row = HStack(spacing: 14) {
            leading
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(Ink.heading).foregroundStyle(enabled ? Ink.paper : Ink.mist)
                if let subtitle {
                    Text(subtitle).font(.footnote).foregroundStyle(subtitleColor)
                }
            }
            Spacer(minLength: 0)
            trailing
            if action != nil, Trailing.self == EmptyView.self {
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(Ink.mist)
            }
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
        if let action {
            Button(action: action) { row }.buttonStyle(.plain).disabled(!enabled)
        } else {
            row
        }
    }
}

extension ListRow where Leading == EmptyView, Trailing == EmptyView {
    init(_ title: String, _ subtitle: String? = nil, enabled: Bool = true, action: (() -> Void)? = nil) {
        self.init(title: title, subtitle: subtitle, enabled: enabled, action: action, leading: { EmptyView() }, trailing: { EmptyView() })
    }
}

/// Заголовок раздела на тёмном экране.
struct SectionTitle: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).font(Ink.title).foregroundStyle(Ink.paper).padding(.top, 12)
    }
}

/// Тёмный экран с круглой кнопкой «назад» и крупным строчным заголовком; содержимое
/// прокручивается.
struct DarkScreen<Content: View>: View {
    let title: String
    var onBack: (() -> Void)?
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 14) {
                    if let onBack {
                        RoundButton(system: "arrow.left", label: "Назад", size: 44, action: onBack)
                    }
                    Text(title).font(Ink.display).foregroundStyle(Ink.paper).lineLimit(2).minimumScaleFactor(0.7)
                }
                .padding(.top, 12)
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.bottom, 24)
        }
        .background(Ink.ink)
        .preferredColorScheme(.dark)
        .tint(Ink.mint)
    }
}

/// Аватар в круге; без картинки — первая буква ника на мятном.
struct AvatarCircle: View {
    let avatar: Data?
    let nick: String?
    var size: CGFloat = 44

    var body: some View {
        if let avatar, !avatar.isEmpty, let image = UIImage(data: avatar) {
            Image(uiImage: image).resizable().frame(width: size, height: size).clipShape(Circle())
        } else {
            ZStack {
                Circle().fill(Ink.accent)
                Text(nick?.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() } ?? "?")
                    .font(.system(size: size * 0.4, weight: .bold))
                    .foregroundStyle(Ink.ink)
            }
            .frame(width: size, height: size)
        }
    }
}

/// Поле ввода на тёмном.
struct DarkField: View {
    let placeholder: String
    @Binding var text: String
    var secure = false
    var keyboard: UIKeyboardType = .default
    var lines: ClosedRange<Int> = 1...1

    var body: some View {
        Group {
            if secure {
                SecureField(placeholder, text: $text)
            } else {
                TextField(placeholder, text: $text, axis: lines.upperBound > 1 ? .vertical : .horizontal)
                    .lineLimit(lines)
            }
        }
        .keyboardType(keyboard)
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .foregroundStyle(Ink.paper)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(Ink.ink2, in: RoundedRectangle(cornerRadius: 16))
    }
}
