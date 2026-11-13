#!/usr/bin/env bash
# Базовая настройка VPS Staya (docs/server.md). Идемпотентна: можно запускать
# повторно. Запуск на сервере от root или через sudo:
#   sudo bash setup-server.sh
#
# ВАЖНО: вход под root и вход по паролю этот скрипт НЕ меняет — прямое указание
# владельца сервера. Не добавлять сюда правки PermitRootLogin/PasswordAuthentication.
set -euo pipefail

DEPLOY_USER="${SUDO_USER:-staya}"
[ "$DEPLOY_USER" != root ] || DEPLOY_USER=staya
APP_DIR=/srv/staya

# Настройки MTU старого провайдера здесь намеренно не применяются.

# --- Обновления ---------------------------------------------------------------
export DEBIAN_FRONTEND=noninteractive
apt-get -qq update
apt-get -y -qq install unattended-upgrades docker.io docker-compose-v2 rsync curl ufw >/dev/null
cat > /etc/apt/apt.conf.d/20auto-upgrades <<'CONF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
CONF
systemctl enable --now unattended-upgrades >/dev/null

# --- Файрвол ------------------------------------------------------------------
# Docker обходит ufw для опубликованных портов: наружу публикует только Caddy
# (сеть хоста), остальные сервисы — на 127.0.0.1 (deploy/docker-compose.yml).
ufw default deny incoming >/dev/null
ufw default allow outgoing >/dev/null
ufw allow OpenSSH >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
ufw --force enable >/dev/null

# Общие настройки Docker не перезаписываем; ротация задана в Compose.
systemctl enable --now docker >/dev/null

# --- Пользователь для деплоя: только ключ, sudo без пароля ---------------------
if ! id "$DEPLOY_USER" >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash "$DEPLOY_USER"
fi

install -d -m 700 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "/home/$DEPLOY_USER/.ssh"
if [ ! -s "/home/$DEPLOY_USER/.ssh/authorized_keys" ] && [ -s /root/.ssh/authorized_keys ]; then
  install -m 600 -o "$DEPLOY_USER" -g "$DEPLOY_USER" /root/.ssh/authorized_keys "/home/$DEPLOY_USER/.ssh/authorized_keys"
fi
echo "$DEPLOY_USER ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/90-$DEPLOY_USER"
chmod 440 "/etc/sudoers.d/90-$DEPLOY_USER"
visudo -cqf "/etc/sudoers.d/90-$DEPLOY_USER"
usermod -aG docker "$DEPLOY_USER"

# --- Каталоги приложения ------------------------------------------------------
install -d -m 750 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$APP_DIR" "$APP_DIR/www"
# Карта (deploy/update-map.sh пишет сюда от имени staya). Создаём заранее: иначе
# Docker при первом запуске создал бы точки монтирования от root.
install -d -m 755 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$APP_DIR/map" "$APP_DIR/map/tiles" "$APP_DIR/map/www"
if [ ! -f "$APP_DIR/www/mtu-64k.bin" ]; then
  head -c 65536 /dev/zero > "$APP_DIR/www/mtu-64k.bin"
fi

# Сервер Staya: пароль базы и код приглашения на регистрацию генерируются здесь
# и не покидают сервер. Код раздаётся друзьям лично (смотреть: sudo cat).
if [ ! -f "$APP_DIR/postgres.env" ]; then
  umask 077
  pw="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  printf 'POSTGRES_PASSWORD=%s\n' "$pw" > "$APP_DIR/postgres.env"
  {
    printf 'STAYA_DATABASE_URL=postgres://staya:%s@postgres:5432/staya\n' "$pw"
    printf 'STAYA_INVITE_CODE=%s\n' "$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  } > "$APP_DIR/server.env"
fi
for f in postgres.env server.env; do
  chown "$DEPLOY_USER:$DEPLOY_USER" "$APP_DIR/$f"
  chmod 600 "$APP_DIR/$f"
done

install -d -m 750 -o "$DEPLOY_USER" -g "$DEPLOY_USER" /srv/proxy /srv/proxy/sites
echo "setup ok"
