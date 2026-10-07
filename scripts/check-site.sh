#!/usr/bin/env bash
# Страница https-ссылок (protocol §5.4): единственный встроенный скрипт должен
# совпадать с хэшем в CSP — иначе браузер его не выполнит (или выполнит чужой).
#   scripts/check-site.sh          # проверить
#   scripts/check-site.sh --fix    # вписать хэш текущего скрипта
set -euo pipefail
page="$(dirname "$0")/../site/index.html"
script="$(awk '/^<script>$/{f=1;next} /^<\/script>$/{f=0} f' "$page")"
[ -n "$script" ] || { echo "no inline script in $page" >&2; exit 1; }
[ "$(grep -c '<script' "$page")" = 1 ] || { echo "exactly one <script> expected" >&2; exit 1; }
# Браузер хэширует текст между тегами, включая переводы строк по краям.
hash="$(printf '\n%s\n' "$script" | openssl dgst -sha256 -binary | openssl base64 -A)"
if [ "${1:-}" = --fix ]; then
  sed -i.bak -E "s#script-src 'sha256-[^']*'#script-src 'sha256-$hash'#" "$page" && rm "$page.bak"
fi
grep -q "script-src 'sha256-$hash'" "$page" || { echo "CSP hash mismatch: expected sha256-$hash (run with --fix)" >&2; exit 1; }
grep -Eq "(src|href)=\"https?://[^\"]*\"" <(grep -v 'github.com/realfamousbae/Staya/blob' "$page") \
  && { echo "external resource in $page" >&2; exit 1; }
echo "site ok"
