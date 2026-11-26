#!/usr/bin/env bash
set -euo pipefail

source_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
build_dir="$source_dir/build"

cmake -S "$source_dir" -B "$build_dir" \
  -DCMAKE_BUILD_TYPE=Debug \
  -DTESTING=OFF
  -DCMAKE_EXPORT_COMPILE_COMMANDS=ON

cmake --build "$build_dir" --config Debug \
  --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-2}"
