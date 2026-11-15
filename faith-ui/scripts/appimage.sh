#!/usr/bin/env bash
set -euo pipefail

ui_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
repo_root=$(cd "$ui_root/.." && pwd)
output_dir=${FAITH_DIST_DIR:-$ui_root/dist}
mkdir -p "$output_dir"
output_dir=$(cd "$output_dir" && pwd)

docker build --file "$ui_root/packaging/appimage/Dockerfile" \
    --tag faith-ui-appimage-builder:local "$repo_root"

container=$(docker create \
    --volume faith-ui-appimage-cache:/cache \
    --env FAITH_APPIMAGE_CACHE=/cache/tools \
    --env CARGO_HOME=/cache/cargo --env CARGO_TARGET_DIR=/cache/target \
    faith-ui-appimage-builder:local)
trap 'docker rm --force "$container" > /dev/null' EXIT

docker start "$container" > /dev/null
docker logs --follow "$container"
status=$(docker wait "$container")

if [[ "$status" != 0 ]]; then
    exit "$status"
fi

docker cp "$container:/dist/." "$output_dir/"
printf 'AppImage and checksum: %s\n' "$output_dir"
