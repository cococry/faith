#!/usr/bin/env bash
set -euo pipefail

ui_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ui_root"

if [[ $(uname -m) != x86_64 ]]; then
    echo 'The AppImage build currently supports Linux x86_64.' >&2
    exit 1
fi

version=$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)
output_dir=${FAITH_DIST_DIR:-$ui_root/dist}
tool_dir=${FAITH_APPIMAGE_CACHE:-$ui_root/.cache/appimage}
mkdir -p "$output_dir" "$tool_dir"
output_dir=$(cd "$output_dir" && pwd)

# Pin the packager and verify it before execution.
tool_version=1-alpha-20251107-1
tool_sha256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
tool="$tool_dir/linuxdeploy-$tool_version-x86_64.AppImage"

if [[ ! -f "$tool" ]]; then
    curl --fail --location --retry 3 \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/$tool_version/linuxdeploy-x86_64.AppImage" \
        --output "$tool.download"
    mv "$tool.download" "$tool"
fi

printf '%s  %s\n' "$tool_sha256" "$tool" | sha256sum --check
chmod +x "$tool"

runtime_version=20251108
runtime_sha256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
runtime="$tool_dir/runtime-$runtime_version-x86_64"

if [[ ! -f "$runtime" ]]; then
    curl --fail --location --retry 3 \
        "https://github.com/AppImage/type2-runtime/releases/download/$runtime_version/runtime-x86_64" \
        --output "$runtime.download"
    mv "$runtime.download" "$runtime"
fi

printf '%s  %s\n' "$runtime_sha256" "$runtime" | sha256sum --check
export LDAI_RUNTIME_FILE="$runtime"

cargo build --locked --release --no-default-features --features appimage --bin faith
binary="${CARGO_TARGET_DIR:-$ui_root/target}/release/faith"
staging=$(mktemp -d "$output_dir/.appimage-build.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT
appdir="$staging/Faith.AppDir"
cargo run --locked --release --no-default-features --features appimage \
    --bin faith-package-icon -- assets/logo.png "$staging/faith.png"

install -d "$appdir/usr/share/faith/assets" "$appdir/usr/share/licenses/faith"
cp -a assets/. "$appdir/usr/share/faith/assets/"
cp -a assets/licenses/. "$appdir/usr/share/licenses/faith/"
cp assets/nerd-fonts/{LICENSE,CODICONS-LICENSE} "$appdir/usr/share/licenses/faith/"
cp packaging/appimage/runtime-LICENSE "$appdir/usr/share/licenses/faith/appimage-runtime-LICENSE"

python3 packaging/appimage/licenses.py "$appdir/usr/share/licenses/faith"

# Keyboard mappings are data, independent of the host's graphics driver stack.
install -d "$appdir/usr/share/X11"
cp -a /usr/share/X11/xkb "$appdir/usr/share/X11/xkb"

export ARCH=x86_64
export APPIMAGE_EXTRACT_AND_RUN=1
export LDAI_COMP=zstd
export LDAI_VERSION="$version"
export LDAI_OUTPUT="$staging/Faith-$version-x86_64.AppImage"
export LDAI_NO_APPSTREAM=1

"$tool" --appimage-extract-and-run \
    --appdir "$appdir" \
    --executable "$binary" \
    --desktop-file packaging/appimage/faith.desktop \
    --icon-file "$staging/faith.png" --icon-filename faith \
    --custom-apprun packaging/appimage/AppRun

# Include notices for the libraries that were actually bundled.
if command -v dpkg-query >/dev/null; then
    install -d "$appdir/usr/share/licenses/system"

    while IFS= read -r library; do
        system_library=$(ldconfig -p | awk -v name="$(basename "$library")" '$1 == name && !found { print $NF; found=1 }')

        if [[ -z "$system_library" ]]; then
            continue
        fi

        package=$(dpkg-query -S "$system_library" "$(readlink -f "$system_library")" 2>/dev/null | head -1 | cut -d: -f1 || true)
        notice="/usr/share/doc/$package/copyright"

        if [[ -f "$notice" ]]; then
            install -Dm644 "$notice" "$appdir/usr/share/licenses/system/$package/copyright"
        fi
    done < <(find "$appdir/usr/lib" -maxdepth 1 -type f -name '*.so*')
fi

"$tool" --appimage-extract-and-run --appdir "$appdir" --output appimage

install -m755 "$LDAI_OUTPUT" "$output_dir/$(basename "$LDAI_OUTPUT")"
cd "$output_dir"
sha256sum "$(basename "$LDAI_OUTPUT")" > "$(basename "$LDAI_OUTPUT").sha256"
printf 'Built %s\n' "$output_dir/$(basename "$LDAI_OUTPUT")"
