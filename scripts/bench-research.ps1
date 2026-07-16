<#
.SYNOPSIS
  Phase 3 gate benchmark for the research pipeline.

.DESCRIPTION
  Builds the offline research-pipeline bench (release), runs fixed fixtures with
  3 warmups + 30 measured runs, computes nearest-rank p50/p95, and writes a JSON
  report containing the commit, OS/CPU, run count, and per-run pipeline timings.

  The offline bench isolates the app's orchestration overhead (no network, no
  model, no generation) — it proves the pipeline-orchestration slice of the
  roadmap targets:
    * cached standard pipeline p95  <= 5000 ms (excluding generation)
    * app overhead p95              <=  500 ms

  Browser-mark gates (first progress <=250 ms p95, Stop <=1 s p95, 100-message
  render <=250 ms p95, one DOM commit/frame) require the RUNNING desktop app over
  the Chrome DevTools Protocol and are NOT fabricated here. Pass -Live with a
  running app (and CDP on :9222) to append those measurements; otherwise the
  report records them as "skipped: requires live app".

.PARAMETER Warmups
  Discarded warmup runs (default 3).

.PARAMETER Measured
  Measured runs for percentiles (default 30).

.PARAMETER Out
  Output JSON path (default: bench-research.json in the repo root).

.PARAMETER Live
  Also probe the running app's browser marks over CDP (opt-in).
#>
param(
    [int]$Warmups = 3,
    [int]$Measured = 30,
    [string]$Out = "bench-research.json",
    [switch]$Live
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Nearest-Rank {
    param([double[]]$Values, [int]$Percentile)
    if ($Values.Count -eq 0) { return $null }
    $sorted = $Values | Sort-Object
    # Nearest-rank: rank = ceil(P/100 * N), 1-indexed.
    $rank = [int][math]::Ceiling($Percentile / 100.0 * $sorted.Count)
    if ($rank -lt 1) { $rank = 1 }
    if ($rank -gt $sorted.Count) { $rank = $sorted.Count }
    return [double]$sorted[$rank - 1]
}

Write-Host "==> Building release bench (offline pipeline)…"
cargo build --manifest-path src-tauri/Cargo.toml --release --example bench_research | Out-Null

$exe = Join-Path $repo "src-tauri/target/release/examples/bench_research.exe"
if (-not (Test-Path $exe)) {
    throw "bench binary not found at $exe"
}

Write-Host "==> Running $Warmups warmups + $Measured measured runs…"
$lines = & $exe --warmups $Warmups --measured $Measured
$cold = @()
foreach ($line in $lines) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    try { $obj = $line | ConvertFrom-Json } catch { continue }
    if ($obj.phase -eq "cold") { $cold += [double]$obj.ms }
}

if ($cold.Count -eq 0) { throw "no measured runs captured" }

$p50 = Nearest-Rank -Values $cold -Percentile 50
$p95 = Nearest-Rank -Values $cold -Percentile 95
$min = ($cold | Measure-Object -Minimum).Minimum
$max = ($cold | Measure-Object -Maximum).Maximum

# Environment provenance.
$commit = (& git rev-parse --short HEAD 2>$null)
if (-not $commit) { $commit = "unknown" }
$cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)
$osName = (Get-CimInstance Win32_OperatingSystem).Caption

# Gate evaluation (offline slice).
$appOverheadOk = $p95 -le 500.0
$pipelineOk = $p95 -le 5000.0

$browser = @{ status = "skipped: requires live app (-Live with CDP :9222)" }
if ($Live) {
    Write-Host "==> -Live set: probing browser marks over CDP…"
    # Placeholder for the CDP probe. Requires the running app exposing
    # performance marks: 'first-progress', 'dom-commit', 'render-100'.
    # See scripts/README or the automated-testing skill for the CDP harness.
    $probe = Join-Path $PSScriptRoot "bench-cdp-probe.ps1"
    if (Test-Path $probe) {
        $browser = & $probe
    } else {
        $browser = @{ status = "skipped: bench-cdp-probe.ps1 not present" }
    }
}

$report = [ordered]@{
    commit          = $commit
    os              = $osName
    cpu             = $cpu
    generated_at    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    warmups         = $Warmups
    measured        = $cold.Count
    pipeline_ms     = [ordered]@{
        p50 = [math]::Round($p50, 4)
        p95 = [math]::Round($p95, 4)
        min = [math]::Round($min, 4)
        max = [math]::Round($max, 4)
    }
    gates           = [ordered]@{
        app_overhead_p95_le_500ms  = $appOverheadOk
        cached_pipeline_p95_le_5s  = $pipelineOk
    }
    browser_marks   = $browser
    note            = "Offline pipeline bench: no network/model/generation. Browser-mark gates need the live app."
}

$json = $report | ConvertTo-Json -Depth 6
$outPath = Join-Path $repo $Out
$json | Set-Content -Path $outPath -Encoding utf8

Write-Host ""
Write-Host $json
Write-Host ""
Write-Host "==> Wrote $outPath"
Write-Host ("==> pipeline p50={0:N3}ms p95={1:N3}ms  app_overhead<=500ms: {2}  pipeline<=5s: {3}" -f $p50, $p95, $appOverheadOk, $pipelineOk)

if (-not ($appOverheadOk -and $pipelineOk)) {
    Write-Error "Offline pipeline gate FAILED (p95=$p95 ms)"
    exit 1
}
Write-Host "==> Offline pipeline gate PASSED"
