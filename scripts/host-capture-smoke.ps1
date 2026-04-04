param(
    [switch]$List,
    [string]$DisplayId,
    [int]$DurationSeconds = 3,
    [int]$TimeoutMs = 16
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$captureDir = Join-Path $repoRoot 'host\capture'
$workspaceTargetDir = Join-Path (Join-Path $repoRoot 'host') 'target'
$binaryPath = Join-Path $workspaceTargetDir 'debug\dxgi_capture_smoke.exe'

Set-Location $captureDir

if (-not (Test-Path $binaryPath)) {
    cargo build --bin dxgi_capture_smoke
}

$arguments = @()
if ($List) {
    $arguments += '--list'
}
if ($DisplayId) {
    $arguments += '--display-id'
    $arguments += $DisplayId
}
$arguments += '--duration-seconds'
$arguments += [string]$DurationSeconds
$arguments += '--timeout-ms'
$arguments += [string]$TimeoutMs

& $binaryPath @arguments
