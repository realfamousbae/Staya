#!/usr/bin/env bash
# Релизный APK (R8, минификация) на эмуляторе: ставится, запускается и живёт.
# При запуске ядро открывается через JNA и UniFFI (AppCore.openAsync) — главное,
# что может сломать минификация, а dexdump этого не покажет. Подпись — отладочным
# ключом (настоящий ключ только на Mac автора).
set -euo pipefail
APK="${1:?release apk}"
PKG=io.github.realfamousbae.staya

adb uninstall "$PKG" >/dev/null 2>&1 || true
adb install -r "$APK"
adb logcat -c
adb shell am start -W -n "$PKG/.MainActivity"
sleep 25
PID="$(adb shell pidof "$PKG" | tr -d '\r' || true)"
CRASH="$(adb logcat -d -b crash || true)"
if [ -z "$PID" ] || echo "$CRASH" | grep -q "$PKG"; then
  echo "release APK crashed or exited" >&2
  echo "$CRASH" | tail -60 >&2
  exit 1
fi
echo "release APK is running (pid $PID)"
