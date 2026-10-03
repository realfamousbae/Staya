#!/usr/bin/env bash
# Базовая настройка VPS Staya (docs/server.md). Идемпотентна: можно запускать
# повторно. Запуск на сервере от root или через sudo:
#   sudo bash setup-server.sh
#
# ВАЖНО: вход под root и вход по паролю этот скрипт НЕ меняет — прямое указание
# владельца сервера. Не добавлять сюда правки PermitRootLogin/PasswordAuthentication.
set -euo pipefail

DEPLOY_USER=staya
APP_DIR=/srv/staya

# --- Сеть: клиенты за VPN -----------------------------------------------------
# Туннели режут ICMP, поиск MTU через ICMP ломается, большие пакеты теряются
# (так зависал SSH на KEX_ECDH_REPLY). Linux сам подбирает размер (RFC 4821).
cat > /etc/sysctl.d/90-staya-net.conf <<'CONF'
net.ipv4.tcp_mtu_probing = 1
net.ipv4.tcp_base_mss = 1024
CONF
sysctl -q --system

# Реальный MTU канала провайдера меньше 1500, а ICMP «нужна фрагментация» до
# сервера не доходит: крупные пакеты с внешних серверов терялись (TLS-рукопожатие
# с ghcr.io и Docker Hub обрывалось в 25–33% попыток, с MTU 1400 — 0 из 30).
# Отдельная служба, а не правка netplan: ошибка в ней не отрежет сервер от сети.
IFACE="$(ip -o -4 route show to default | awk '{print $5; exit}')"
cat > /etc/systemd/system/staya-mtu.service <<UNIT
[Unit]
Description=Lower MTU on the uplink (provider path MTU < 1500)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/sbin/ip link set ${IFACE} mtu 1400
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now staya-mtu.service >/dev/null

# --- Обновления ---------------------------------------------------------------
export DEBIAN_FRONTEND=noninteractive
apt-get -qq update
apt-get -y -qq install unattended-upgrades docker.io docker-compose-v2 rsync >/dev/null
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

# --- Docker: ротация логов контейнеров, чтобы не забить диск -------------------
mkdir -p /etc/docker
DAEMON_JSON='{"log-driver":"json-file","log-opts":{"max-size":"10m","max-file":"3"}}'
if [ "$(cat /etc/docker/daemon.json 2>/dev/null)" != "$DAEMON_JSON" ]; then
  echo "$DAEMON_JSON" > /etc/docker/daemon.json
  systemctl restart docker
fi
systemctl enable --now docker >/dev/null

# --- Пользователь для деплоя: только ключ, sudo без пароля ---------------------
if ! id "$DEPLOY_USER" >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash "$DEPLOY_USER"
fi
passwd -l "$DEPLOY_USER" >/dev/null
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
# Сборщик метрик работает в distroless как nonroot (uid 65532).
install -d -m 700 -o 65532 -g 65532 "$APP_DIR/probe-data"
if [ ! -f "$APP_DIR/www/mtu-64k.bin" ]; then
  head -c 65536 /dev/zero > "$APP_DIR/www/mtu-64k.bin"
fi
# Токен сборщика: генерируется здесь и не покидает сервер (смотреть: sudo cat).
if [ ! -f "$APP_DIR/probe.env" ]; then
  umask 077
  printf 'PROBE_TOKEN=%s\n' "$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')" > "$APP_DIR/probe.env"
fi
chown "$DEPLOY_USER:$DEPLOY_USER" "$APP_DIR/probe.env"
chmod 600 "$APP_DIR/probe.env"

echo "setup ok"
