#!/usr/bin/env bash

set -euo pipefail

echo "Checking formatting..."
cargo fmt --all -- --check

echo "Running clippy..."
cargo clippy --locked --all-targets --all-features -- -D warnings

echo "Running tests..."
cargo test --locked --all-targets --all-features --no-fail-fast

echo "Building docs..."
cargo doc --locked --no-deps --all-features

echo "All checks passed."
