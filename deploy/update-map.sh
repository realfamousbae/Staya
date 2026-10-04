#!/usr/bin/env bash
# Обновление карты на сервере с Mac (задача 3.9): стили из репозитория, вырезка
# региона, шрифты и спрайты — на самом сервере (тяжёлое скачивание идёт оттуда).
#   STAYA_HOST=user@host STAYA_DOMAIN=имя deploy/update-map.sh
set -euo pipefail

HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
DOMAIN="${STAYA_DOMAIN:?set STAYA_DOMAIN to the server name}"
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
DIR="$(cd "$(dirname "$0")" && pwd)"
SSH=(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5)

"${SSH[@]}" "$HOST" "install -d /srv/staya/map-src"
rsync -az -e "${SSH[*]}" "$DIR/map/style-light.json" "$DIR/map/style-dark.json" "$HOST:/srv/staya/map-src/"
"${SSH[@]}" "$HOST" "STAYA_DOMAIN='$DOMAIN' bash -s" < "$DIR/map/update-map.sh"
# Сервер тайлов перечитывает архив только при старте.
"${SSH[@]}" "$HOST" "cd /srv/staya && docker compose up -d tiles && docker compose restart tiles"
