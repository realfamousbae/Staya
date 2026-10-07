#!/usr/bin/env bash
# Ссылка на сервер для первого пользователя (protocol §5.3, §5.4): имя, отпечатки
# ключей TLS и код регистрации — открыть её на телефоне, и вводить ничего не нужно.
# Уже зарегистрированное приложение по этой ссылке (после подтверждения) запомнит
# код и начнёт класть его в свои приглашения.
#   STAYA_HOST=user@host STAYA_DOMAIN=имя STAYA_BACKUP_PIN=<запасной> deploy/server-link.sh
# STAYA_BACKUP_PIN — отпечаток запасного ключа (deploy/pin.sh <файл ключа>), можно
# не указывать. Код читается с сервера и выводится только внутри ссылки: ссылка —
# пропуск на регистрацию, её не публикуют.
set -euo pipefail

HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
DOMAIN="${STAYA_DOMAIN:?set STAYA_DOMAIN to the server name for TLS}"
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
DIR="$(cd "$(dirname "$0")" && pwd)"

pins="$(STAYA_DOMAIN="$DOMAIN" "$DIR/pin.sh")"
[ -n "${STAYA_BACKUP_PIN:-}" ] && pins="$pins.$STAYA_BACKUP_PIN"

code=""
for i in 1 2 3 4 5; do
  if code="$(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10 "$HOST" \
      "sudo sed -n 's/^STAYA_INVITE_CODE=//p' /srv/staya/server.env")"; then break; fi
  [ "$i" = 5 ] && { echo "cannot reach $HOST" >&2; exit 1; }
  sleep 3
done

link="server?v=1&s=$DOMAIN&p=$pins"
if [ -n "$code" ]; then
  # В ссылку помещается только такой код (protocol §5.3).
  if ! printf '%s' "$code" | grep -Eq '^[A-Za-z0-9_-]{1,40}$'; then
    echo "STAYA_INVITE_CODE does not fit in a link: use [A-Za-z0-9_-], up to 40 characters" >&2
    exit 1
  fi
  link="$link&c=$code"
fi
echo "https://realfamousbae.github.io/Staya/#$link"
