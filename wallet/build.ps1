# Build the DontYeetWallet wasm-bindgen bundle.
#
# Output: frontend/wallet/pkg/  (consumed by index.html as an ES module)
#
# Usage:
#   pwsh .\build.ps1            # release build, ~250-400 KB gzipped
#   pwsh .\build.ps1 -Dev       # dev build, faster compile, larger binary
#
# Requires: wasm-pack (cargo install wasm-pack), rustup target add wasm32-unknown-unknown.

param(
    [switch]$Dev
)

$ErrorActionPreference = "Stop"

$ScriptRoot = $PSScriptRoot
$OutDir = Join-Path (Split-Path -Parent $ScriptRoot) "frontend/wallet/pkg"

$Mode = if ($Dev) { "--dev" } else { "--release" }

Write-Host "Building DontYeetWallet ($Mode) -> $OutDir"

wasm-pack build $ScriptRoot `
    $Mode `
    --target web `
    --out-dir $OutDir `
    --out-name wallet

if ($LASTEXITCODE -ne 0) {
    Write-Error "wasm-pack build failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}

Write-Host "Build complete."
Write-Host "Bundle: $OutDir"
