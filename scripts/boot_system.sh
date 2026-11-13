#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
instances=${1:-1}
case "$instances" in ''|*[!0-9]*) echo 'Usage: scripts/boot_system.sh [1..16]' >&2; exit 1;; esac
[ "$instances" -ge 1 ] && [ "$instances" -le 16 ] || exit 1
command -v docker >/dev/null
command -v openssl >/dev/null
umask 077
mkdir -p .local/system .local/clients
for secret in db-admin-password db-app-password; do
  if [ ! -s ".local/system/$secret" ]; then openssl rand -hex 32 > ".local/system/$secret"; fi
  chmod 444 ".local/system/$secret"
done
if [ ! -s .local/system/key.pem ] || [ ! -s .local/system/cert.pem ] ||
   ! openssl x509 -checkend 86400 -noout -in .local/system/cert.pem >/dev/null 2>&1; then
  chmod u+w .local/system/key.pem .local/system/cert.pem 2>/dev/null || true
  openssl req -x509 -newkey rsa:3072 -nodes -days 30 -subj /CN=localhost \
    -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' \
    -keyout .local/system/key.pem -out .local/system/cert.pem
fi
chmod 444 .local/system/key.pem .local/system/cert.pem
cat > .local/system.env <<ENV
FAITH_DB_ADMIN_PASSWORD_FILE=$root/.local/system/db-admin-password
FAITH_DB_APP_PASSWORD_FILE=$root/.local/system/db-app-password
FAITH_TLS_CERT_FILE=$root/.local/system/cert.pem
FAITH_TLS_KEY_FILE=$root/.local/system/key.pem
FAITH_BIND_ADDRESS=127.0.0.1
FAITH_PORT_RANGE=4433-4448
FAITH_API_IMAGE=faith-api:local
ENV
# Stop old API versions before applying a schema change; keep the DB volume.
docker compose --env-file .local/system.env stop api
docker compose --env-file .local/system.env up -d --build --wait db
# Re-run the migration job on every boot. Its ledger makes repeated runs safe.
docker compose --env-file .local/system.env run --build --rm --no-deps migrate
docker compose --env-file .local/system.env up -d --no-deps --scale "api=$instances" --wait api
for project in faith-proto faith-client faith-client/test; do
  (cd "$project" && cc -o nob nob.c && ./nob)
done
printf '\nSystem ready.' 
i=1
while [ "$i" -le "$instances" ]; do
  docker compose --env-file .local/system.env port --index "$i" api 4433
  i=$((i + 1))
done
printf '\nDONE, lets go.' 
