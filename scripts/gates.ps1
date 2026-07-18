# Deterministic automated gates — plan Task 9.2.
# Runs the four required gate commands in order and exits non-zero on the first
# failure, so "automated gates" is encoded on disk, not just in a transcript.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gates.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host "== [1/4] finmodel-core workspace ==" -ForegroundColor Cyan
    cargo test --manifest-path finmodel-core/Cargo.toml --workspace
    if ($LASTEXITCODE -ne 0) { throw "core workspace tests failed" }

    Write-Host "== [2/4] src-tauri lib ==" -ForegroundColor Cyan
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    if ($LASTEXITCODE -ne 0) { throw "src-tauri lib tests failed" }

    Write-Host "== [3/4] ui ==" -ForegroundColor Cyan
    npm test --prefix ui
    if ($LASTEXITCODE -ne 0) { throw "ui tests failed" }

    Write-Host "== [4/4] research-eval hard gate ==" -ForegroundColor Cyan
    cargo test --manifest-path finmodel-core/Cargo.toml -p fm-research --test research_eval -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "research-eval gate failed" }

    Write-Host "ALL GATES GREEN" -ForegroundColor Green
} finally {
    Pop-Location
}
