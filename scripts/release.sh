#!/usr/bin/env bash
# Бета-релиз (задача 4.8): с Mac автора, где лежит ключ подписи (release-key.sh).
#   scripts/release.sh 0.2.0 [файл-с-заметками.md]
# Собирает подписанный APK здесь и неподписанный IPA в CI (тот же коммит), кладёт
# их с SHA256SUMS в ЧЕРНОВИК релиза GitHub. Публикует автор, проверив черновик:
#   gh release edit v0.2.0 --draft=false
set -euo pipefail

VERSION="${1:?версия, например 0.2.0}"
NOTES="${2:-}"
[[ "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || { echo "версия вида 0.2.0" >&2; exit 1; }
# Код версии растёт с каждой версией: 0.2.0 → 200, 1.3.12 → 10312.
CODE=$((BASH_REMATCH[1] * 10000 + BASH_REMATCH[2] * 100 + BASH_REMATCH[3]))
TAG="v$VERSION"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
KS="$HOME/.staya-release/staya-release.p12"
[ -f "$KS" ] || { echo "нет ключа: сначала scripts/release-key.sh" >&2; exit 1; }

# Только чистый main, совпадающий с GitHub: релиз = ровно этот коммит.
git fetch -q origin
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "нужна ветка main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "есть незакоммиченные изменения" >&2; exit 1; }
SHA="$(git rev-parse HEAD)"
[ "$SHA" = "$(git rev-parse origin/main)" ] || { echo "main не совпадает с origin/main" >&2; exit 1; }
if gh release view "$TAG" >/dev/null 2>&1; then echo "релиз $TAG уже есть" >&2; exit 1; fi

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# IPA — в CI (локальный Xcode не ставит на свежую iOS), запускаем сразу: собирается параллельно.
echo "== IPA в CI"
# Принимаем только новый запуск: ручной запуск на том же коммите дал бы IPA с версией по умолчанию.
# Пока идёт релиз, ios-ipa вручную не запускать — тот же ref отменил бы этот запуск.
BEFORE="$(gh run list --workflow ios-ipa.yml -L 50 --json databaseId -q '.[].databaseId' | sort)"
gh workflow run ios-ipa.yml --ref main -f version="$VERSION" -f build="$CODE"
RUN=""
for _ in $(seq 1 30); do
  sleep 5
  RUN="$(gh run list --workflow ios-ipa.yml --branch main --event workflow_dispatch -L 5 \
    --json databaseId,headSha -q ".[] | select(.headSha == \"$SHA\") | .databaseId" \
    | sort | comm -13 <(echo "$BEFORE") - | tail -1)"
  [ -n "$RUN" ] && break
done
[ -n "$RUN" ] || { echo "не нашёл новый запуск ios-ipa для $SHA" >&2; exit 1; }

echo "== APK"
source scripts/env.sh >/dev/null
# Пароль — из связки ключей macOS в окружение сборки; на экран и в файлы не выводится.
STAYA_KEYSTORE_PASSWORD="$(security find-generic-password -a "$USER" -s staya-apk-signing -w)"
export STAYA_KEYSTORE="$KS" STAYA_KEYSTORE_PASSWORD
(cd android && ./gradlew -q --no-daemon clean assembleRelease \
  -PstayaVersionName="$VERSION" -PstayaVersionCode="$CODE")
unset STAYA_KEYSTORE_PASSWORD
APK="android/app/build/outputs/apk/release/app-release.apk"
CERT="$("$ANDROID_HOME/build-tools/36.0.0/apksigner" verify --print-certs "$APK" | grep -i "SHA-256" | head -1 | sed 's/.*: //')"
[ -n "$CERT" ] || { echo "APK не подписан" >&2; exit 1; }
echo "сертификат APK, SHA-256: $CERT"
cp "$APK" "$OUT/Staya-$VERSION.apk"

echo "== ждём IPA (run $RUN)"
gh run watch "$RUN" --exit-status >/dev/null
gh run download "$RUN" --dir "$OUT/ipa"
cp "$(find "$OUT/ipa" -name Staya.ipa | head -1)" "$OUT/Staya-$VERSION.ipa"
# Версия внутри IPA — та, что в релизе (по ней AltStore видит обновление).
unzip -p "$OUT/Staya-$VERSION.ipa" Payload/Staya.app/Info.plist > "$OUT/Info.plist"
[ "$(plutil -extract CFBundleShortVersionString raw "$OUT/Info.plist")" = "$VERSION" ] \
  && [ "$(plutil -extract CFBundleVersion raw "$OUT/Info.plist")" = "$CODE" ] \
  || { echo "версия в IPA не $VERSION ($CODE)" >&2; exit 1; }

(cd "$OUT" && shasum -a 256 "Staya-$VERSION.apk" "Staya-$VERSION.ipa" > SHA256SUMS)
cat "$OUT/SHA256SUMS"

if [ -z "$NOTES" ]; then
  NOTES="$OUT/notes.md"
  cat > "$NOTES" <<NOTES_EOF
Бета Staya $VERSION. Как установить и обновлять — [docs/beta.md](https://github.com/realfamousbae/Staya/blob/$TAG/docs/beta.md).

- Android: \`Staya-$VERSION.apk\` (через Obtainium — обновления сами).
- iPhone: \`Staya-$VERSION.ipa\` через AltStore или SideStore своим Apple ID.
- Проверка файлов: \`SHA256SUMS\`.
- Отпечаток сертификата подписи APK (SHA-256): \`$CERT\` — одинаковый у всех версий.
NOTES_EOF
fi

gh release create "$TAG" --target "$SHA" --draft --title "Staya $VERSION" --notes-file "$NOTES" \
  "$OUT/Staya-$VERSION.apk" "$OUT/Staya-$VERSION.ipa" "$OUT/SHA256SUMS"
echo
echo "Черновик $TAG создан. Проверь на GitHub и опубликуй: gh release edit $TAG --draft=false"
