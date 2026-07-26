#!/usr/bin/env bash
# Thin wrapper kept for CI and muscle memory. The CLI now owns the wasm
# runtime build itself (cargo build -p aria-web + wasm-bindgen, cached under
# target/aria-web-runtime/release); this script only pins the historical
# arguments: project dir first, output dir second, signed release profile.
set -euo pipefail

project="${1:-examples/v3-minimal}"
output="${2:-${project%/}/dist/web}"

cargo run --locked --release -p aria-cli -- build "$project" \
  --target web --profile signed --release --out "$output"

echo "Runnable V3 Web package: $output"
