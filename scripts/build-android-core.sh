#!/usr/bin/env bash
# Собирает Rust-ядро в .so для Android и генерирует Kotlin-привязки.
# Использование: scripts/build-android-core.sh [debug|release]   (по умолчанию release)
# Вызывается из Gradle (модуль :core) перед сборкой.
set -euo pipefail

PROFILE="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODULE="$ROOT/android/core"
JNI="$MODULE/build/rust/jniLibs"
KOTLIN="$MODULE/build/rust/kotlin"
# Только arm64: все целевые телефоны и эмулятор на Apple Silicon — arm64.
ABIS=(arm64-v8a)

: "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME не задан — source scripts/env.sh}"

cd "$ROOT"
rustup target add aarch64-linux-android >/dev/null

CARGO_FLAGS=(-p staya-core --lib)
[[ "$PROFILE" == "release" ]] && CARGO_FLAGS+=(--release)
ABI_FLAGS=()
for a in "${ABIS[@]}"; do ABI_FLAGS+=(-t "$a"); done

rm -rf "$JNI" "$KOTLIN"
cargo ndk "${ABI_FLAGS[@]}" --platform 29 -o "$JNI" build "${CARGO_FLAGS[@]}"

cargo build -q -p uniffi-bindgen
target/debug/uniffi-bindgen generate --language kotlin --no-format \
  --library "$JNI/arm64-v8a/libstaya_core.so" \
  --out-dir "$KOTLIN"
echo "OK: $JNI, $KOTLIN"
