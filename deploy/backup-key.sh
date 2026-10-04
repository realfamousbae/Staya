#!/usr/bin/env bash
# Запасной ключ TLS сервера (protocol §5.3). Создаётся один раз на своём компьютере
# и НЕ кладётся ни на сервер, ни в репозиторий: перенести в менеджер паролей и
# удалить файл. В приглашения идёт только отпечаток, он печатается в конце.
#   deploy/backup-key.sh [файл]     # по умолчанию ~/staya-backup-key.pem
set -euo pipefail

OUT="${1:-$HOME/staya-backup-key.pem}"
[ -e "$OUT" ] && { echo "$OUT already exists" >&2; exit 1; }
umask 077
# P-256, как ключи, которые выпускает Caddy по умолчанию.
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$OUT"
echo "key: $OUT (move it to your password manager, then delete the file)"
printf 'pin: '
"$(dirname "$0")/pin.sh" "$OUT"
