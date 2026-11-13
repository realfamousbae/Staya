# Свой сервер Staya: несколько проектов на VPS

Новая установка: Ubuntu 24.04, от 2 ГБ RAM, SSH по ключу и sudo, Docker Compose.
На Mac для скриптов нужны Python 3 и OpenSSL.
Старую установку этим способом не обновлять: её сертификаты находятся в другом томе.
Для неё нужен отдельный перенос TLS-ключей, конфигурации и данных.

## Перед запуском

Проверить `uname -m`, `free -h`, `df -h`, `ss -lntup`, работающие контейнеры,
ОС и правила firewall. Публикуемый CI образ сейчас рассчитан на x86_64.
При наличии других проектов сначала согласовать существующий прокси и порты.
Указать локально (не сохранять адреса и секреты в публичном репозитории):

```bash
export STAYA_HOST=user@host
export STAYA_KEY=/path/to/ssh-key
export STAYA_DOMAIN=<ip-через-дефисы>.sslip.io
export SERVER_TAG=<12-символьный-SHA-проверенного-образа>
```

## Первоначальная установка

1. `deploy/bootstrap.sh` — один раз на новом VPS. Устанавливает Docker, ufw,
   автообновления, создаёт каталоги и секреты Staya, запускает общий Caddy.
   Вход root и вход по паролю не меняет. MTU и конфигурацию Docker не заменяет.
   Открывает OpenSSH, 80 и 443/tcp; при нестандартном SSH-порте сначала разрешить его.
2. `deploy/deploy.sh` — запускает API, PostgreSQL и тайлы, проверяет API с базой,
   добавляет сайт Staya и проверяет конфигурацию Caddy перед reload.
3. `deploy/update-map.sh` — загружает карту Москвы и области (~600 МБ), стили,
   шрифты и спрайты. Регион меняется в `deploy/map/update-map.sh`.
4. Изначально Caddy использует тестовый ACME. После проверки выпуска записать
   `ACME_CA=https://acme-v02.api.letsencrypt.org/directory` в `/srv/proxy/.env`,
   затем на VPS выполнить `cd /srv/proxy && docker compose up -d`.
   Проверить доверенную TLS-цепочку извне без `curl -k`.
5. Создать запасной TLS-ключ через `deploy/backup-key.sh`, сохранить его в менеджере
   паролей. Получить ссылку через `STAYA_BACKUP_PIN=<отпечаток> deploy/server-link.sh`.
   Эта ссылка содержит код регистрации: отправлять лично, не публиковать.

## Разделение проектов

- `/srv/proxy`: Compose-проект `proxy`, общий Caddy, отдельный постоянный том TLS.
- `/srv/staya`: Compose-проект `staya`, API и тайлы на loopback 8082/8083,
  PostgreSQL без опубликованного порта, отдельные том и сеть.
- Для нового проекта: собственный `/srv/<project>`, уникальное имя Compose,
  свободный loopback-порт и файл `/srv/proxy/sites/<project>.caddy` со своим доменом.
  После добавления проверить `caddy validate`, затем `caddy reload` внутри контейнера.
  Не менять глобальные TLS-политики других сайтов. Для Staya сохранять TLS 1.3,
  `reuse_private_keys` и отсутствие access-логов.
- Секреты Staya — `server.env`, `postgres.env`, права 600. Общий прокси их не монтирует.
- Логи ротируются на уровне контейнеров. Лимиты памяти по умолчанию: Caddy 128 МБ,
  API 512 МБ, PostgreSQL 384 МБ, тайлы 256 МБ; это пределы, а не резервирование RAM.
  На VPS проверить `docker stats --no-stream`, OOM/restarts и свободную память.
  При необходимости изменить `*_MEMORY_LIMIT` в соответствующем `.env` перед запуском
  следующего проекта. CPU разделяется планировщиком, гарантированной квоты нет.

## Обновление и откат

Собирать образ в CI, дождаться проверок и задать его точный 12-символьный SHA-тег.
`deploy/deploy.sh` не настраивает ОС и не перезапускает Docker или соседние проекты.
Он не переносит домен существующей установки. Последняя конфигурация сохраняется
в `.env.previous` и `compose.previous.yml`; не повторять неудачный деплой до разбора
ошибки, чтобы не затереть эти копии.

Перед обновлением работающей базы скрипт сохраняет `pg_dump` в `/srv/staya/backups`
с правами 600. Контролировать свободный диск и удалять ненужные копии вручную. Откат бинарника
допустим только при совместимой схеме: миграции применяются при старте, старый сервер
откажется работать с более новой схемой. При совместимой схеме вернуть `.env.previous`
и `compose.previous.yml` на их обычные имена и выполнить `docker compose up -d` из
`/srv/staya`. При несовместимой — нужен согласованный откат базы с остановкой API.
Не удалять тома командой `down -v`. TLS-том сохранять отдельно: потеря ключа ломает pinning.

## Приёмка

Проверить извне `/health` (API и БД), WebSocket, стили и тайлы, закрытые 8082/8083/5432;
после перезагрузки VPS — автозапуск и прежний TLS-отпечаток. Добавить временный второй
сайт и проверить его доступность при обновлении и остановке Staya.
На двух телефонах: регистрация, QR/ссылка, дружба, карта, позиция, призрак,
заморозка, удаление; отдельно фон с заблокированным экраном и восстановление сети.
Проверки на VPS и телефонах отмечать отдельно от локальных тестов.

## Optional FCM for Android

FCM is opt-in and does not carry message contents or locations. For your own server,
build the Android app with a matching Firebase project's `google-services.json`
(ignored by git). The main CI uses the encrypted `STAYA_FIREBASE_ANDROID_CONFIG`
secret for the mobile configuration only; never put a service-account key there.
Without this configuration or Play Services, ordinary mailbox/location sync remains.

Use a dedicated Google service account with a custom project role containing only
`cloudmessaging.messages.create`. Do not export the broad default Admin SDK account.
The server key never belongs in the APK, repository, CI or map/web directories.

Provision it with `deploy/configure-fcm.sh /private/service-account.json` using the
normal SSH environment. Optionally set `STAYA_FCM_EXPECTED_PROJECT` to verify the
project against the mobile configuration. The script creates a private read-only
runtime mount owned by the container UID 65532 and adds the credential-file path to
`server.env`; it does not restart Docker or other projects. It refuses to overwrite
an existing key; rotation requires an explicit separate operation.

Deploy the tested image normally. On the VPS, from `/srv/staya`, verify:

```bash
docker compose exec -T server /staya-server --check-fcm
```

This uses Google's validation mode and sends no notification. It proves configured
credentials and sending access, not delivery to a real phone. Then enable Google
push updates in the Android app and verify reception on that device. iOS remains
without APNs until a separate developer-account setup.

The SDK keeps auto-init disabled. Explicit consent enables bounded mailbox jobs
and token refresh; disabling it removes the server address and invalidates the SDK
token, retrying cleanup when offline. Google sees an opaque installation/delivery
identifier and delivery times. The Firebase SDK manages its own registration
metadata in the app sandbox; Staya account/session/E2E keys retain their existing
Keystore/encrypted-store protections. Analytics, Crashlytics, advertising IDs and
delivery-metrics export are not enabled.
