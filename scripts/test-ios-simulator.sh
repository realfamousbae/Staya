#!/usr/bin/env bash
# Самотест приложения на iOS-симуляторе (CI): настоящий Keychain и ядро.
# Локально нужен скачанный рантайм iOS-симулятора (около 8 ГБ) — поэтому только в CI.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
DERIVED="$(mktemp -d)"

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
echo "$OUT" | grep SELFTEST || true
echo "$OUT" | grep -q "SELFTEST OK"
