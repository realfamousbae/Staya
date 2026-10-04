#!/usr/bin/env bash
# Выполняется НА СЕРВЕРЕ (запускает deploy/update-map.sh с Mac): вырезка карты
# региона из свежей сборки Protomaps, шрифты и спрайты, стили с адресом сервера.
#   STAYA_DOMAIN=имя bash update-map.sh
# Регион — Москва и Московская область (~600 МБ при максимальном масштабе 15).
set -euo pipefail

: "${STAYA_DOMAIN:?STAYA_DOMAIN}"
APP_DIR=/srv/staya
MAP="$APP_DIR/map"
BBOX="35.10,54.20,40.25,57.00"
# go-pmtiles v1.31.2
PMTILES="protomaps/go-pmtiles:v1.31.2@sha256:06574f01f55a78f78f887bc7ebf729a5c093c0d6e17d9876300cfcb0758b59d3"
# protomaps/basemaps-assets на 04.10.2026: шрифты Noto Sans (OFL) и спрайты.
ASSETS_SHA=028c18f713baecad011301ff7a69acc39bcc2ae7

install -d -m 755 "$MAP/tiles" "$MAP/www"

# Последняя сборка планеты из списка Protomaps.
BUILD="$(curl -fsS --retry 5 https://build-metadata.protomaps.dev/builds.json \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)[-1]["key"])')"
echo "build: $BUILD"

# Скачиваются только тайлы региона (HTTP range). Во временный файл: работающий
# сервер тайлов продолжает отдавать старую карту, пока новая не готова.
for i in 1 2 3; do
  docker run --rm --user "$(id -u):$(id -g)" -v "$MAP/tiles:/out" "$PMTILES" extract \
    "https://build.protomaps.com/$BUILD" /out/region.pmtiles.new \
    --bbox="$BBOX" --download-threads=4 && break
  [ "$i" = 3 ] && { echo "extract failed" >&2; exit 1; }
  sleep 10
done
mv "$MAP/tiles/region.pmtiles.new" "$MAP/tiles/region.pmtiles"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL --retry 5 "https://codeload.github.com/protomaps/basemaps-assets/tar.gz/$ASSETS_SHA" \
  | tar -xz -C "$TMP"
SRC="$TMP/basemaps-assets-$ASSETS_SHA"
NEW="$MAP/www.new"
rm -rf "$NEW" && install -d "$NEW/fonts" "$NEW/sprites"
for font in "Noto Sans Regular" "Noto Sans Medium" "Noto Sans Italic"; do
  cp -r "$SRC/fonts/$font" "$NEW/fonts/"
done
cp "$SRC/fonts/OFL.txt" "$NEW/fonts/"
for flavor in light dark; do
  cp "$SRC/sprites/v4/$flavor".* "$SRC/sprites/v4/$flavor"@2x.* "$NEW/sprites/"
  sed "s|{{BASE}}|https://$STAYA_DOMAIN|g" "$APP_DIR/map-src/style-$flavor.json" > "$NEW/style-$flavor.json"
done
chmod -R a+rX "$NEW"
rm -rf "$MAP/www.old" && { [ -d "$MAP/www" ] && mv "$MAP/www" "$MAP/www.old" || true; }
mv "$NEW" "$MAP/www" && rm -rf "$MAP/www.old"
du -sh "$MAP/tiles/region.pmtiles" "$MAP/www"
