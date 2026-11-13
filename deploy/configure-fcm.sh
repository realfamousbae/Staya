#!/usr/bin/env bash
# Provision a dedicated send-only Google service-account key, never a client SDK file.
# STAYA_HOST=user@host STAYA_KEY=/path/to/ssh-key deploy/configure-fcm.sh /private/key.json
# Does not restart anything; deploy a tested FCM-capable image afterwards.
set -euo pipefail
HOST="${STAYA_HOST:?set STAYA_HOST=user@host}"
KEY="${STAYA_KEY:-$HOME/.ssh/staya_vps}"
CREDENTIALS="${1:?pass the private service-account JSON path}"
python3 - "$CREDENTIALS" <<'PY'
import json, os, sys
with open(sys.argv[1]) as source:
    value = json.load(source)
if value.get('type') != 'service_account' or not value.get('private_key') or not value.get('project_id'):
    raise SystemExit('Expected a service-account credential file')
expected = os.environ.get('STAYA_FCM_EXPECTED_PROJECT')
if expected and value['project_id'] != expected:
    raise SystemExit('Credential project does not match the configured mobile project')
PY
SSH=(ssh -i "$KEY" -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5)
"${SSH[@]}" "$HOST" 'test -f /srv/staya/server.env && sudo -n test ! -e /srv/staya/secrets/fcm-service-account.json'
UPLOAD="$("${SSH[@]}" "$HOST" 'mktemp /srv/staya/.fcm-upload.XXXXXXXX')"
[[ "$UPLOAD" =~ ^/srv/staya/\.fcm-upload\.[A-Za-z0-9]+$ ]] || exit 1
# A fixed, private generated path prevents credentials appearing in command arguments.
"${SSH[@]}" "$HOST" "cat > '$UPLOAD'" < "$CREDENTIALS"
"${SSH[@]}" "$HOST" "bash -s -- '$UPLOAD'" <<'REMOTE'
set -euo pipefail
cd /srv/staya
exec 9>.deploy.lock
flock 9
sudo -n install -d -m 700 -o 65532 -g 65532 secrets
sudo -n test ! -e secrets/fcm-service-account.json
sudo -n install -m 400 -o 65532 -g 65532 "$1" secrets/fcm-service-account.json
rm "$1"
umask 077
{ grep -v '^STAYA_FCM_CREDENTIALS=' server.env || true; printf 'STAYA_FCM_CREDENTIALS=/run/secrets/fcm-service-account.json\n'; } > server.env.fcm-next
sudo -n chown --reference=server.env server.env.fcm-next
chmod 600 server.env.fcm-next
mv server.env.fcm-next server.env
echo 'FCM key provisioned privately. Deploy the tested image, then run its --check-fcm dry run.'
REMOTE
