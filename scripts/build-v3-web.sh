#!/usr/bin/env bash
# Thin wrapper kept for CI and muscle memory. The CLI now owns the wasm
# runtime build itself (cargo build -p aria-web + wasm-bindgen, cached under
# target/aria-web-runtime/release); this script only pins the historical
# arguments: project dir first, output dir second, signed release profile.
set -euo pipefail

project="${1:-examples/v3-minimal}"
output="${2:-${project%/}/dist/web}"

# The CLI enables the native player by default for desktop packaging.  A web
# package never needs that adapter, and keeping it out avoids host-only audio
# dependencies (such as ALSA) leaking into Web/WASM builds.
cargo run --locked --release -p aria-cli --no-default-features -- build "$project" \
  --target web --profile signed --release --out "$output"

echo "Runnable V3 Web package: $output"
