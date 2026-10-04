# Staya

Открытое приложение для обмена геопозицией с друзьями со сквозным шифрованием.
План и чекбоксы задач: docs/PLAN.md · Протокол: docs/protocol.md · Модель угроз: docs/threat-model.md

## Структура
- proto/ — staya-proto: форматы конвертов, API-типы (без криптографии)
- core/ — staya-core: vodozemac (Olm + Megolm), протокол, хранилище, UniFFI; sans-IO — сеть на стороне платформ
- server/ — staya-server: Rust (axum), PostgreSQL; правила ящиков — `mailbox.rs`, dev-сервер в памяти — фича `dev` (`cargo run -p staya-server --features dev --bin staya-dev-server`)
- tools/dev-peer — тестовый собеседник на ядре для dev-сервера (CI: обмен с симулятором и эмулятором)
- ios/ — Swift, SwiftUI, iOS 18+
- android/ — Kotlin, Jetpack Compose, minSdk 29, compileSdk/targetSdk 37, без Google Play Services

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
- Android SDK минимальный: platform-tools, platforms;android-37.0, build-tools;36.0.0, ndk;29.0.14206865. Ничего сверх этого без согласования.
- iOS: сборка без подписи — `xcodebuild -project ios/Staya.xcodeproj -target Staya -sdk iphonesimulator CODE_SIGNING_ALLOWED=NO build` (схемы с -destination требуют скачанной iOS-платформы). DEVELOPMENT_TEAM — в ios/Config/Local.xcconfig (не в git).
- iOS: Personal Team (нет платного Apple Developer Program) — без APNs, TestFlight, universal links.

## Сборка ядра для приложений
- iOS: `scripts/build-ios-core.sh` → ios/StayaCore/StayaCoreFFI.xcframework + Swift-привязки (не в git). Запускать перед сборкой Xcode после изменений в core/.
- Android: `./gradlew assembleDebug` сам вызывает `scripts/build-android-core.sh` (нужен `source scripts/env.sh`). Только arm64-v8a.
- Экспорт в Swift/Kotlin — через `#[uniffi::export]` (proc-macros, без UDL).

## Проверки перед коммитом
- cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo test, cargo deny check
- Android: `./gradlew assembleDebug lintDebug testDebugUnitTest` (lint — 0 замечаний)
- CI (.github/workflows/ci.yml): новые Actions закреплять по SHA коммита с комментарием версии
