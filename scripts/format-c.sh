#!/usr/bin/env bash
# Copyright (c) 2026 Luca Machiedo
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORMATTER="${CLANG_FORMAT:-clang-format}"
STYLE="$ROOT/faith-server/.clang-format"

if ! command -v "$FORMATTER" >/dev/null 2>&1; then
  echo "Required formatter not found: $FORMATTER" >&2
  exit 1
fi

case "${1:-}" in
  "") FLAGS=(-i) ;;
  --check) FLAGS=(--dry-run --Werror) ;;
  *) echo "Usage: $0 [--check]" >&2; exit 1 ;;
esac

if (( $# > 1 )); then
  echo "Usage: $0 [--check]" >&2
  exit 1
fi

# Use the established server style for all three projects, including headers,
# build definitions and the TUI, without touching dependencies or build output.
while IFS= read -r -d '' file; do
  "$FORMATTER" --style="file:$STYLE" "${FLAGS[@]}" "$file"
done < <(
  find "$ROOT/faith-proto" "$ROOT/faith-client" "$ROOT/faith-server" \
    -type d \( -name build -o -name target -o -name third_party -o -name .git \) -prune -o \
    -type f \( -name '*.c' -o -name '*.h' \) -print0
)

echo "C source and header formatting complete."
