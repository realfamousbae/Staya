#!/usr/bin/env bash
# Отпечаток ключа TLS для приглашений (protocol §5.3): SHA-256 от
# SubjectPublicKeyInfo в base64url без выравнивания.
#   STAYA_DOMAIN=имя deploy/pin.sh          # ключ, который сервер предъявляет сейчас
#   deploy/pin.sh /путь/к/ключу.pem         # ключ из файла (запасной)
set -euo pipefail

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

if [ $# -ge 1 ]; then
  openssl pkey -in "$1" -pubout -outform DER | openssl dgst -sha256 -binary | b64url
else
  DOMAIN="${STAYA_DOMAIN:?set STAYA_DOMAIN or pass a key file}"
  openssl s_client -connect "$DOMAIN:443" -servername "$DOMAIN" </dev/null 2>/dev/null \
    | openssl x509 -pubkey -noout \
    | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | b64url
fi
echo
