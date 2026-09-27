# Staya

Открытое приложение для обмена геопозицией с друзьями со сквозным шифрованием.
План и чекбоксы задач: docs/PLAN.md · Протокол: docs/protocol.md · Модель угроз: docs/threat-model.md

## Структура
- proto/ — staya-proto: форматы конвертов, API-типы (без криптографии)
- core/ — staya-core: vodozemac (Olm + Megolm), протокол, хранилище, UniFFI; sans-IO — сеть на стороне платформ
- server/ — staya-server: Rust (axum), PostgreSQL
- ios/ — Swift, SwiftUI, iOS 18+
- android/ — Kotlin, Jetpack Compose, minSdk 29, без Google Play Services

## Правила безопасности (обязательны)
- Не писать собственную криптографию: только vodozemac и проверенные примитивы (RustCrypto).
- Координаты никогда не логируются, не уходят на сервер в открытом виде и не попадают в аналитику. Типы с координатами реализуют Debug как `<redacted>`.
- Секреты хранятся только в Keychain / Android Keystore, никогда в UserDefaults, SharedPreferences или файлах.
- Новая зависимость добавляется только с обоснованием в описании изменения.
- Изменение протокола: сначала docs/protocol.md, затем код и тесты.
- Никаких SDK аналитики, рекламы и трекинга. В MVP нет пушей (ни APNs, ни FCM).

## Процесс
- Одна задача из docs/PLAN.md — одна ветка `stageN/короткое-имя` — один PR в main. После мержа отметить чекбокс.
- Коммиты без строк атрибуции.

## Окружение
- `source scripts/env.sh` — JAVA_HOME (openjdk@21) и ANDROID_HOME (~/Library/Android/sdk, без Android Studio).
- Android SDK минимальный: platform-tools, platforms;android-36, build-tools;36.0.0, ndk;29.0.14206865. Ничего сверх этого без согласования.
- iOS: Personal Team (нет платного Apple Developer Program) — без APNs, TestFlight, universal links.

## Проверки перед коммитом
- cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo test, cargo deny check
