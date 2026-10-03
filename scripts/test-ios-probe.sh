#!/usr/bin/env bash
# Проверки кода замеров iOS без устройства (macOS):
#  - ProbeQueue: запись, пришедшая во время отправки, не теряется;
#  - контракт ProbeRecord ↔ tools/probe-server (локальный сервер с тестовым токеном).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap '[ -n "${SERVER_PID:-}" ] && { kill "$SERVER_PID" 2>/dev/null; wait "$SERVER_PID" 2>/dev/null || true; }; rm -rf "$TMP"' EXIT
cd "$ROOT"

SRC=(ios/Staya/Probe/ProbeRecord.swift ios/Staya/Probe/ProbeQueue.swift)
swiftc -D STAYA_PROBE "${SRC[@]}" ios/ProbeTests/Queue/main.swift -o "$TMP/queue"
"$TMP/queue"

cargo build -q -p probe-server
TOKEN="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
mkdir -p "$TMP/data"
PROBE_TOKEN="$TOKEN" PROBE_LISTEN=127.0.0.1:18080 PROBE_DATA_DIR="$TMP/data" target/debug/probe-server 2>/dev/null &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:18080/health && break; sleep 0.1; done

swiftc -D STAYA_PROBE ios/Staya/Probe/ProbeRecord.swift ios/ProbeTests/Contract/main.swift -o "$TMP/contract"
PROBE_URL=http://127.0.0.1:18080/probe PROBE_TOKEN="$TOKEN" "$TMP/contract"
