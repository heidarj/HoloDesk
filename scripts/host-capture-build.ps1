param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$captureDir = Join-Path $repoRoot 'host\capture'

Set-Location $captureDir

cargo build --bins
