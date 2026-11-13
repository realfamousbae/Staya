#!/usr/bin/env bash
# Обновляет только Staya и её сайт в общем Caddy. Начальная настройка: bootstrap.sh.
# STAYA_HOST=user@host STAYA_DOMAIN=name SERVER_TAG=<12-char commit> deploy/deploy.sh
set -euo pipefail
HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
DOMAIN="${STAYA_DOMAIN:?set STAYA_DOMAIN}"
TAG="${SERVER_TAG:?set SERVER_TAG to a tested 12-character commit tag}"
[[ "$DOMAIN" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]] || { echo 'invalid domain' >&2; exit 1; }
[[ "$TAG" =~ ^[0-9a-f]{12}$ ]] || { echo 'SERVER_TAG must be a 12-character commit SHA' >&2; exit 1; }
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
DIR="$(cd "$(dirname "$0")" && pwd)"
SSH=(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5)
"${SSH[@]}" "$HOST" 'test -f /srv/proxy/docker-compose.yml && test -f /srv/staya/server.env'
STAGE="$("${SSH[@]}" "$HOST" 'mktemp -d /srv/staya/.deploy.XXXXXXXX')"
[[ "$STAGE" =~ ^/srv/staya/\.deploy\.[A-Za-z0-9]+$ ]] || exit 1
rsync -az -e "${SSH[*]}" "$DIR/docker-compose.yml" "$HOST:$STAGE/compose.next.yml"
rsync -az -e "${SSH[*]}" "$DIR/staya.caddy" "$HOST:$STAGE/site.next"
"${SSH[@]}" "$HOST" "bash -s -- '$DOMAIN' '$TAG' '$STAGE'" <<'REMOTE'
set -euo pipefail
cd /srv/staya
exec 9>.deploy.lock
flock -n 9 || { echo 'another deployment is running' >&2; exit 1; }
domain=$1; tag=$2; stage=$3
mv "$stage/compose.next.yml" compose.next.yml
mv "$stage/site.next" site.next
rmdir "$stage"
if [ -f .env ]; then
  old_domain=$(sed -n 's/^STAYA_DOMAIN=//p' .env)
  [ "$old_domain" = "$domain" ] || { echo 'domain change requires explicit migration' >&2; exit 1; }
fi
export STAYA_DOMAIN="$domain" SERVER_TAG="$tag"
docker compose -f compose.next.yml config -q
docker compose -f compose.next.yml pull
# Save the database before startup can apply migrations; never print its contents.
if [ -f docker-compose.yml ] && docker compose ps --status running --services | grep -qx postgres; then
  install -d -m 700 backups
  dump="backups/before-${tag}-$(date -u +%Y%m%dT%H%M%SZ).sql"
  (umask 077; docker compose exec -T postgres pg_dump -U staya staya </dev/null > "$dump")
fi
# Retain last successful configuration and version (no data-volume deletion).
[ ! -f .env ] || cp .env .env.previous
[ ! -f docker-compose.yml ] || cp docker-compose.yml compose.previous.yml
{ if [ -f .env ]; then sed '/^STAYA_DOMAIN=/d; /^SERVER_TAG=/d' .env; fi
  printf 'STAYA_DOMAIN=%s\nSERVER_TAG=%s\n' "$domain" "$tag"; } > .env.next
chmod 600 .env.next
mv .env.next .env
mv compose.next.yml docker-compose.yml
docker compose up -d --remove-orphans
ready=0
for i in {1..30}; do
  if curl -fsS http://127.0.0.1:8082/health >/dev/null; then ready=1; break; fi
  sleep 2
done
[ "$ready" = 1 ] || { echo 'API not ready; see logs and rollback instructions' >&2; exit 1; }
exec 8>/srv/proxy/.reload.lock
flock 8
sed "s/{\$STAYA_DOMAIN}/$domain/g" site.next > /srv/proxy/sites/staya.candidate
cd /srv/proxy
[ ! -f sites/staya.caddy ] || cp sites/staya.caddy sites/staya.previous
mv sites/staya.candidate sites/staya.caddy
if ! { docker compose exec -T caddy caddy validate --config /etc/caddy/Caddyfile </dev/null &&
       docker compose exec -T caddy caddy reload --config /etc/caddy/Caddyfile </dev/null; }; then
  if [ -f sites/staya.previous ]; then mv sites/staya.previous sites/staya.caddy; else rm sites/staya.caddy; fi
  exit 1
fi
cd /srv/staya
docker compose ps
echo 'API ready. Check public TLS, map and two-device delivery before accepting deployment.'
REMOTE
