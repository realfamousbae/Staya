#!/usr/bin/env bash
# Правило доверия iOS (ServerTrustEvaluator) на Mac без симулятора: тестовый CA и
# сертификаты генерирует openssl, отпечатки SPKI для сверки — тоже openssl.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"

openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
  -subj "/CN=Staya test CA" -keyout ca.key -out ca.pem \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" 2>/dev/null
leaf() { # имя файла, имя в сертификате
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj "/CN=$2" \
    -keyout "$1.key" -out "$1.csr" 2>/dev/null
  printf 'subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature\nbasicConstraints=critical,CA:FALSE\n' "$2" > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA ca.pem -CAkey ca.key -CAcreateserial -days 30 \
    -extfile "$1.ext" -out "$1.pem" 2>/dev/null
  openssl x509 -in "$1.pem" -outform DER -out "$1.der"
  openssl x509 -in "$1.pem" -pubkey -noout | openssl pkey -pubin -outform DER \
    | openssl dgst -sha256 -binary | openssl base64 -A > "$1.pin"
}
leaf leaf localhost
leaf leaf2 localhost
leaf other other.example
openssl x509 -in ca.pem -outform DER -out ca.der

swiftc -swift-version 6 "$ROOT/ios/Staya/Net/ServerTrustEvaluator.swift" \
  "$ROOT/ios/CoreTests/Trust/sha256.swift" "$ROOT/ios/CoreTests/Trust/main.swift" -o trust-test
./trust-test "$TMP"
