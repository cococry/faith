#!/usr/bin/env bash
set -euo pipefail

image=$(realpath "${1:?Usage: smoke-appimage.sh path/to/Faith.AppImage}")
workspace=$(mktemp -d)
trap 'rm -rf -- "$workspace"' EXIT

cd "$workspace"
"$image" --appimage-extract > /dev/null
mv squashfs-root relocated
mkdir home data
"$workspace/relocated/AppRun" --help > /dev/null

if LD_LIBRARY_PATH="$workspace/relocated/usr/lib" ldd "$workspace/relocated/usr/bin/faith" | grep -q 'not found'; then
    echo 'The AppImage has unresolved libraries.' >&2
    exit 1
fi

export HOME="$workspace/home"
export XDG_DATA_HOME="$workspace/data"
unset WAYLAND_DISPLAY XKB_CONFIG_ROOT
export LIBGL_ALWAYS_SOFTWARE=1
export LP_NUM_THREADS=2

timeout 30s xvfb-run -a -s '-screen 0 1280x720x24 +extension GLX' \
    bash -eu -o pipefail -c '
        "$1/relocated/AppRun" --platform x11 --graphics opengl \
            --ready-file "$1/first-frame-ready" > "$1/startup.log" 2>&1 &
        ui_pid=$!
        trap '\''kill "$ui_pid" 2>/dev/null || true; wait "$ui_pid" 2>/dev/null || true'\'' EXIT

        for ((attempt = 0; attempt < 200; attempt++)); do
            if [[ -f "$1/first-frame-ready" ]]; then
                printf "First-frame RSS (KiB, software renderer): "
                ps -o rss= -p "$ui_pid"
                cat "$1/startup.log"
                exit 0
            fi

            if ! kill -0 "$ui_pid" 2>/dev/null; then
                cat "$1/startup.log" >&2
                exit 1
            fi

            sleep 0.1
        done

        cat "$1/startup.log" >&2
        echo "Timed out waiting for the first rendered frame." >&2
        exit 1
    ' bash "$workspace"

echo 'Relocated AppImage startup and rendering passed.'
