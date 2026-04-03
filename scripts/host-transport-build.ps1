param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

Set-Location $transportDir

cargo build --bins
