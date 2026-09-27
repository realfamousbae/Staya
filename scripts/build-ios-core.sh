#!/usr/bin/env bash
# Собирает Rust-ядро в StayaCore.xcframework и генерирует Swift-привязки.
# Использование: scripts/build-ios-core.sh [debug|release]   (по умолчанию release)
set -euo pipefail

PROFILE="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/ios/StayaCore"
OUT="$PKG/Generated"
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)

cd "$ROOT"
rustup target add "${TARGETS[@]}" >/dev/null

CARGO_FLAGS=(-p staya-core --lib)
[[ "$PROFILE" == "release" ]] && CARGO_FLAGS+=(--release)
for t in "${TARGETS[@]}"; do
  cargo build "${CARGO_FLAGS[@]}" --target "$t"
done

# Привязки генерируются из собранной библиотеки (library mode UniFFI).
cargo build -q -p uniffi-bindgen
rm -rf "$OUT" && mkdir -p "$OUT/headers" "$OUT/swift"
target/debug/uniffi-bindgen generate --language swift --no-format \
  --library "target/aarch64-apple-ios/$PROFILE/libstaya_core.a" \
  --out-dir "$OUT/bindgen"
mv "$OUT"/bindgen/*.swift "$OUT/swift/"
mv "$OUT"/bindgen/*.h "$OUT/headers/"
mv "$OUT"/bindgen/*.modulemap "$OUT/headers/module.modulemap"
rm -rf "$OUT/bindgen"

rm -rf "$PKG/StayaCoreFFI.xcframework"
xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/$PROFILE/libstaya_core.a" -headers "$OUT/headers" \
  -library "target/aarch64-apple-ios-sim/$PROFILE/libstaya_core.a" -headers "$OUT/headers" \
  -output "$PKG/StayaCoreFFI.xcframework" >/dev/null

# Swift-исходник привязок кладём в цель пакета.
mkdir -p "$PKG/Sources/StayaCore"
cp "$OUT"/swift/*.swift "$PKG/Sources/StayaCore/"
echo "OK: $PKG/StayaCoreFFI.xcframework"
