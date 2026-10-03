#!/usr/bin/env bash
# Деплой с Mac: копирует конфиги и перезапускает сервисы (docs/server.md).
#   deploy/deploy.sh [сервис ...]      # без аргументов — все сервисы
# Через туннели новые SSH-соединения часто обрываются, поэтому одно постоянное
# соединение (ControlMaster) и повторы.
set -euo pipefail

HOST="${STAYA_HOST:-staya@2.27.42.60}"
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

"${SSH[@]}" "$HOST" "cd $APP_DIR && docker compose pull --quiet $* && docker compose up -d --remove-orphans $* && docker compose ps"
