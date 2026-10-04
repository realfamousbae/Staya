#!/usr/bin/env bash
# Деплой с Mac: копирует конфиги и перезапускает сервисы (docs/server.md).
#   STAYA_HOST=user@host STAYA_DOMAIN=имя deploy/deploy.sh [сервис ...]
# Без сервисов — все. Адрес и имя сервера в репозитории не хранятся (он публичный):
# STAYA_HOST — куда заходить по SSH, STAYA_DOMAIN — имя для TLS, его deploy.sh
# записывает в /srv/staya/.env на сервере.
# Через туннели новые SSH-соединения часто обрываются, поэтому одно постоянное
# соединение (ControlMaster) и повторы.
set -euo pipefail

HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
DOMAIN="${STAYA_DOMAIN:?set STAYA_DOMAIN to the server name for TLS}"
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
APP_DIR=/srv/staya
DIR="$(cd "$(dirname "$0")" && pwd)"
CTL="$(mktemp -d /tmp/staya-ssh.XXXX)/c"
SSH=(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5
     -o ServerAliveCountMax=3 -o ControlMaster=auto -o ControlPath="$CTL" -o ControlPersist=5m)
trap '"${SSH[@]}" -O exit "$HOST" >/dev/null 2>&1 || true' EXIT

for i in 1 2 3 4 5 6; do
  "${SSH[@]}" "$HOST" true && break
  [ "$i" = 6 ] && { echo "cannot reach $HOST" >&2; exit 1; }
  sleep 5
done

# Сначала базовая настройка (идемпотентна), затем конфиги.
"${SSH[@]}" "$HOST" "sudo bash -s" < "$DIR/setup-server.sh"
rsync -az -e "${SSH[*]}" "$DIR/docker-compose.yml" "$DIR/Caddyfile" "$HOST:$APP_DIR/"
# Имя сервера — в .env на сервере (права 600), одна строка STAYA_DOMAIN.
"${SSH[@]}" "$HOST" "cd $APP_DIR && touch .env && chmod 600 .env && \
  { grep -v '^STAYA_DOMAIN=' .env > .env.new || true; } && echo 'STAYA_DOMAIN=$DOMAIN' >> .env.new && mv .env.new .env && chmod 600 .env"

# Исходящие соединения с VPS к реестрам иногда сбоят — повторяем скачивание.
for i in 1 2 3 4; do
  "${SSH[@]}" "$HOST" "cd $APP_DIR && docker compose pull --quiet $*" && break
  [ "$i" = 4 ] && { echo "image pull failed" >&2; exit 1; }
  sleep 10
done
"${SSH[@]}" "$HOST" "cd $APP_DIR && docker compose up -d --remove-orphans $* && docker compose ps"
