#!/usr/bin/env bash
set -euo pipefail

./scripts/build-appimage.sh

# Hide the repository assets so the rendering check can only use the bundle.
hidden=$(mktemp -d)
mv assets "$hidden/ui-assets"
mv /source/assets "$hidden/root-assets"
./scripts/smoke-appimage.sh /dist/*.AppImage
