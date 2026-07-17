[CmdletBinding()]
param(
    [string]$Project = "examples/v3-minimal",
    [string]$Output,
    [string]$RuntimeDir = "target/aria-web-runtime"
)

$ErrorActionPreference = "Stop"
if (-not $Output) { $Output = Join-Path $Project "dist/web" }
if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)) {
    throw "wasm-bindgen CLI is required (matching wasm-bindgen 0.2.126)"
}

cargo build --locked --release -p aria-web --target wasm32-unknown-unknown --features web-gpu
if ($LASTEXITCODE -ne 0) { throw "aria-web WASM build failed" }
New-Item -ItemType Directory -Force -Path $RuntimeDir | Out-Null
wasm-bindgen --target web --out-dir $RuntimeDir --out-name aria_web target/wasm32-unknown-unknown/release/aria_web.wasm
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen packaging failed" }

$previousRuntimeDir = $env:ARIA_WEB_RUNTIME_DIR
try {
    $env:ARIA_WEB_RUNTIME_DIR = $RuntimeDir
    cargo run --locked --release -p aria-cli -- build $Project --target web --profile signed --release --out $Output
    if ($LASTEXITCODE -ne 0) { throw "aria web packaging failed" }
} finally {
    $env:ARIA_WEB_RUNTIME_DIR = $previousRuntimeDir
}

Write-Host "Runnable V3 Web package: $Output"
