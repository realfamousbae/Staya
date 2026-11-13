#!/usr/bin/env bash
# Один раз на НОВОМ Ubuntu 24.04 VPS. Не запускать на прежнем сервере Staya.
set -euo pipefail
HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
DIR="$(cd "$(dirname "$0")" && pwd)"
SSH=(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10)
# Осмотр до любых изменений: образы CI сейчас только x86_64.
"${SSH[@]}" "$HOST" 'bash -s' <<'REMOTE'
set -euo pipefail
. /etc/os-release
[ "$ID" = ubuntu ] && [ "$VERSION_ID" = 24.04 ]
[ "$(uname -m)" = x86_64 ]
free -h
df -h /
if ss -ltnH | awk '{print $4}' | grep -Eq ':(80|443)$'; then
  echo '80/443 already occupied: inspect existing proxy first' >&2
  exit 1
fi
REMOTE
# Не заменять существующую установку прокси или Staya.
"${SSH[@]}" "$HOST" 'test ! -e /srv/proxy/docker-compose.yml && test ! -e /srv/staya/docker-compose.yml'
"${SSH[@]}" "$HOST" 'sudo bash -s' < "$DIR/setup-server.sh"
rsync -az -e "${SSH[*]}" "$DIR/Caddyfile" "$HOST:/srv/proxy/"
rsync -az -e "${SSH[*]}" "$DIR/proxy-compose.yml" "$HOST:/srv/proxy/docker-compose.yml"
"${SSH[@]}" "$HOST" 'cd /srv/proxy && sudo docker compose config -q && sudo docker compose up -d'
echo 'Proxy ready (staging CA). Next: deploy.sh, update-map.sh, then enable production ACME and generate server link; see docs/self-hosting.md.'
