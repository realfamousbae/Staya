#!/usr/bin/env bash
# Самотест приложения на iOS-симуляторе (CI): настоящий Keychain и ядро, затем
# обмен позициями с тестовым собеседником (tools/dev-peer) через dev-сервер (2.10).
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
OUT="$(xcrun simctl launch --console-pty --terminate-running-process "$UDID" "$BUNDLE" -staya-selftest 2>&1 || true)"
echo "$OUT" | grep -q "SELFTEST OK" || { echo "self-test failed, app output:"; echo "$OUT"; exit 1; }
echo "SELFTEST OK"

# Обмен с dev-peer. Симулятор делит сеть с хостом: 127.0.0.1 — это Mac.
cargo build -q -p staya-server --features dev --bin staya-dev-server
cargo build -q -p dev-peer
target/debug/staya-dev-server > "$LOGS/dev-server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:8787/dev/invite && break; sleep 0.2; done
# Координаты должны совпадать с SelfTest.swift.
target/debug/dev-peer --server http://127.0.0.1:8787 --role invite \
  --mine 557558000,376173000 --expect 599386000,303141000 --timeout 600 \
  > "$LOGS/dev-peer.log" 2>&1 &
PEER_PID=$!
OUT="$(xcrun simctl launch --console-pty --terminate-running-process "$UDID" "$BUNDLE" \
  -staya-devexchange http://127.0.0.1:8787 2>&1 || true)"
echo "$OUT" | grep -q "DEVEXCHANGE OK" || { echo "dev exchange failed, app output:"; echo "$OUT"; exit 1; }
echo "DEVEXCHANGE OK"
wait "$PEER_PID"
grep -q "PEER GOT LOCATION" "$LOGS/dev-peer.log"
echo "dev exchange: both sides got the other's location"
