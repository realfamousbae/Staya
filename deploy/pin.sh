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
  # Python performs a verified handshake and closes cleanly; some macOS OpenSSL
  # builds return a poll error on stdin EOF even after a successful handshake.
  python3 - "$DOMAIN" <<'PYTLS' | openssl x509 -pubkey -noout \
    | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | b64url
import socket, ssl, sys
context = ssl.create_default_context()
context.minimum_version = ssl.TLSVersion.TLSv1_3
with socket.create_connection((sys.argv[1], 443), timeout=15) as conn:
    with context.wrap_socket(conn, server_hostname=sys.argv[1]) as tls:
        print(ssl.DER_cert_to_PEM_cert(tls.getpeercert(binary_form=True)))
PYTLS
fi
echo
