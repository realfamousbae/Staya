# Staya

Открытое мобильное приложение для iOS и Android, чтобы делиться геопозицией с небольшим кругом друзей. Координаты шифруются на устройстве сквозным шифрованием (Olm/Megolm через [vodozemac](https://github.com/matrix-org/vodozemac)), сервер хранит только шифротекст последнего пакета.

> Статус: ранняя разработка, этап 0. Пользоваться пока нельзя.

## Принципы

- Сервер не видит координат: даже при полном доступе к базе и трафику.
- Без номера телефона и email, без истории перемещений.
- Свой сервер, открытые карты (OpenStreetMap), никаких SDK аналитики и рекламы.

## Документы

- [План проекта](docs/PLAN.md)
- [Протокол](docs/protocol.md) и [модель угроз](docs/threat-model.md) — черновики
- [Сообщить об уязвимости](SECURITY.md)

## Структура

| Папка | Что внутри |
| --- | --- |
| `proto/` | Форматы конвертов и API-типы (Rust) |
| `core/` | Криптография, протокол, локальное хранилище (Rust + UniFFI) |
| `server/` | Сервер почтовых ящиков (Rust, axum, PostgreSQL) |
| `ios/` | Приложение iOS (Swift, SwiftUI) |
| `android/` | Приложение Android (Kotlin, Jetpack Compose) |

## Сборка

```bash
cargo build                      # Rust: proto, core, server
source scripts/env.sh            # JAVA_HOME и ANDROID_HOME для мобильных сборок
scripts/build-ios-core.sh        # ядро для iOS, затем открыть ios/Staya.xcodeproj
(cd android && ./gradlew assembleDebug)   # APK, ядро собирается автоматически
```

## Лицензия

[AGPL-3.0](LICENSE).
