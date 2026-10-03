#!/usr/bin/env bash
# Проверки кода замеров Android без устройства: JVM-тесты кодирования и очереди и
# контракт ProbeRecord ↔ tools/probe-server (локальный сервер с тестовым токеном).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap '[ -n "${SERVER_PID:-}" ] && { kill "$SERVER_PID" 2>/dev/null; wait "$SERVER_PID" 2>/dev/null || true; }; rm -rf "$TMP"' EXIT
cd "$ROOT"

cargo build -q -p probe-server
TOKEN="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
mkdir -p "$TMP/data"
PROBE_TOKEN="$TOKEN" PROBE_LISTEN=127.0.0.1:18081 PROBE_DATA_DIR="$TMP/data" target/debug/probe-server 2>/dev/null &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:18081/health && break; sleep 0.1; done

cd android
PROBE_URL=http://127.0.0.1:18081/probe PROBE_TOKEN="$TOKEN" \
  ./gradlew --no-daemon -q :app:testDebugUnitTest --rerun-tasks -Dorg.gradle.configuration-cache=false
echo "contract lines stored: $(cat "$TMP"/data/*.jsonl | wc -l | tr -d ' ')"
