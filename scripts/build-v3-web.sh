#!/usr/bin/env bash
set -euo pipefail

project="${1:-examples/v3-minimal}"
output="${2:-${project%/}/dist/web}"
runtime_dir="${ARIA_WEB_RUNTIME_BUILD_DIR:-target/aria-web-runtime}"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen CLI is required (matching wasm-bindgen 0.2.126)" >&2
  exit 2
fi

cargo build --locked --release -p aria-web --target wasm32-unknown-unknown --features web-gpu
mkdir -p "$runtime_dir"
wasm-bindgen \
  --target web \
  --out-dir "$runtime_dir" \
  --out-name aria_web \
  target/wasm32-unknown-unknown/release/aria_web.wasm

ARIA_WEB_RUNTIME_DIR="$runtime_dir" \
  cargo run --locked --release -p aria-cli -- build "$project" --target web --profile signed --release --out "$output"

echo "Runnable V3 Web package: $output"
