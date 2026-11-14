#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
name=${1:?Usage: scripts/run_ui.sh PROFILE_NAME [PORT] [UI_OPTIONS...]}
shift
case "$name" in ''|*[!a-zA-Z0-9_-]*) echo 'Profile names may contain letters, digits, _ and - only.' >&2; exit 1;; esac
[ -f .local/system/cert.pem ] || { echo 'Run scripts/boot_system.sh first.' >&2; exit 1; }
umask 077
mkdir -p .local/clients
port=
if [ "$#" -gt 0 ]; then
    case "$1" in *[!0-9]*|'') ;; *) port=$1; shift;; esac
fi
if [ -z "$port" ]; then
    endpoint=$(docker compose --env-file .local/system.env port --index 1 api 4433)
    port=${endpoint##*:}
fi
exec cargo run --manifest-path faith-ui/Cargo.toml --locked -- \
    --profile "$root/.local/clients/$name" --host 127.0.0.1 --port "$port" \
    --ca-file "$root/.local/system/cert.pem" --server-name localhost "$@" 
