#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
name=${1:?Usage: scripts/run_tui.sh PROFILE_NAME [PORT]}
case "$name" in ''|*[!a-zA-Z0-9_-]*) echo 'Profile names may contain letters, digits, _ and - only.' >&2; exit 1;; esac
[ -x faith-client/test/build/faith-tui ] || { echo 'Run scripts/boot_system.sh first.' >&2; exit 1; }
[ -f .local/system/cert.pem ] || { echo 'Run scripts/boot_system.sh first.' >&2; exit 1; }
umask 077
mkdir -p .local/clients
port=${2:-}
if [ -z "$port" ]; then
  endpoint=$(docker compose --env-file .local/system.env port --index 1 api 4433)
  port=${endpoint##*:}
fi
exec faith-client/test/build/faith-tui "$root/.local/clients/$name" 127.0.0.1 "$port" "$root/.local/system/cert.pem" localhost
