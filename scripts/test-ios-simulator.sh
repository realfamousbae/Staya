#!/usr/bin/env bash
# Самотест приложения на iOS-симуляторе (CI): настоящий Keychain и ядро, затем
# обмен позициями с тестовым собеседником (tools/dev-peer --login) через настоящий
# сервер Staya с PostgreSQL и входом (4.1c). Адрес базы — STAYA_EXCHANGE_DATABASE_URL.
# Локально нужен скачанный рантайм iOS-симулятора (около 8 ГБ) — поэтому только в CI.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
DERIVED="$(mktemp -d)"
LOGS="${STAYA_LOG_DIR:-$DERIVED/logs}"
mkdir -p "$LOGS"
trap 'kill ${SERVER_PID:-} ${PEER_PID:-} 2>/dev/null || true' EXIT

UDID="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
ios = sorted((r for r in devices if "iOS" in r), reverse=True)
for runtime in ios:
    for d in devices[runtime]:
        if d["name"].startswith("iPhone"):
            print(d["udid"]); sys.exit()
sys.exit("no iPhone simulator")
')"
echo "simulator: $UDID"
xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b > /dev/null

# Подпись «для локального запуска» (ad hoc): без неё Keychain в симуляторе
# может отвечать -34018 (нет entitlements).
xcodebuild -project ios/Staya.xcodeproj -scheme Staya -configuration Debug \
  -destination "id=$UDID" -derivedDataPath "$DERIVED" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= \
  build | tail -n 5
test "${PIPESTATUS[0]}" -eq 0

APP="$DERIVED/Build/Products/Debug-iphonesimulator/Staya.app"
BUNDLE="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$APP/Info.plist")"
xcrun simctl install "$UDID" "$APP"
# Первый запуск на свежем симуляторе бывает медленным: до трёх попыток.
for attempt in 1 2 3; do
  OUT="$(xcrun simctl launch --console-pty --terminate-running-process "$UDID" "$BUNDLE" -staya-selftest 2>&1 || true)"
  echo "$OUT" | grep -q "SELFTEST OK" && break
  echo "self-test attempt $attempt: no result"
done
echo "$OUT" | grep -q "SELFTEST OK" || { echo "self-test failed, app output:"; echo "$OUT"; exit 1; }
echo "SELFTEST OK"

# Обмен через настоящий сервер. Симулятор делит сеть с хостом: 127.0.0.1 — это Mac.
: "${STAYA_EXCHANGE_DATABASE_URL:?set STAYA_EXCHANGE_DATABASE_URL}"
cargo build -q -p staya-server --bin staya-server
cargo build -q -p dev-peer
STAYA_DATABASE_URL="$STAYA_EXCHANGE_DATABASE_URL" STAYA_DOMAIN=staya.test STAYA_LISTEN=127.0.0.1:8080 \
  target/debug/staya-server > "$LOGS/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -sf -o /dev/null http://127.0.0.1:8080/health && break; sleep 0.2; done
INVITE_FILE="$DERIVED/invite"
# Координаты должны совпадать с SelfTest.swift.
target/debug/dev-peer --server http://127.0.0.1:8080 --login staya.test --invite-file "$INVITE_FILE" \
  --role invite --mine 557558000,376173000 --expect 599386000,303141000 --timeout 600 \
  > "$LOGS/dev-peer.log" 2>&1 &
PEER_PID=$!
for _ in $(seq 1 50); do [ -s "$INVITE_FILE" ] && break; sleep 0.2; done
OUT="$(xcrun simctl launch --console-pty --terminate-running-process "$UDID" "$BUNDLE" \
  -staya-devexchange http://127.0.0.1:8080 "$(cat "$INVITE_FILE")" 2>&1 || true)"
echo "$OUT" | grep -q "DEVEXCHANGE OK" || { echo "dev exchange failed, app output:"; echo "$OUT"; exit 1; }
echo "DEVEXCHANGE OK"
wait "$PEER_PID"
grep -q "PEER GOT LOCATION" "$LOGS/dev-peer.log"
echo "exchange through the server: both sides got the other's location"
