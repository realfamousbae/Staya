#!/usr/bin/env bash
# Один раз на Mac автора: ключ подписи релизных APK (задача 4.8). Ключ не покидает
# Mac и не попадает ни в git, ни в CI. Пароль вводится руками (keytool спросит
# дважды) и затем ещё раз — для связки ключей macOS, откуда его берёт release.sh.
# Потеря ключа = друзьям переустанавливать приложение; утечка = кто-то сможет
# выпустить «обновление». Поэтому: копия файла и пароля — в менеджер паролей.
set -euo pipefail

DIR="$HOME/.staya-release"
KS="$DIR/staya-release.p12"
SERVICE=staya-apk-signing

if [ -e "$KS" ]; then
  echo "Ключ уже есть: $KS — второй не создаём (обновления перестали бы ставиться)." >&2
  exit 1
fi
source "$(dirname "$0")/env.sh" >/dev/null
install -d -m 700 "$DIR"

echo "Создаю ключ. Придумай длинный пароль — keytool спросит его дважды."
"$JAVA_HOME/bin/keytool" -genkeypair -storetype PKCS12 -keystore "$KS" -alias staya \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Staya"
chmod 600 "$KS"

echo "Теперь тот же пароль — для связки ключей macOS (оттуда его берёт release.sh)."
security add-generic-password -U -a "$USER" -s "$SERVICE" -T /usr/bin/security -w

echo
echo "Готово: $KS"
echo "Отпечаток сертификата (публичный — его можно публиковать для проверки APK):"
# Пароль — через окружение, не аргументом: аргументы видны в списке процессов.
STAYA_KS_PASS="$(security find-generic-password -a "$USER" -s "$SERVICE" -w)" \
  "$JAVA_HOME/bin/keytool" -list -keystore "$KS" -alias staya -storepass:env STAYA_KS_PASS \
  | grep -i "SHA-256" || true
echo
echo "Сохрани в менеджер паролей файл $KS и пароль. Без них новые версии не встанут поверх старых."
