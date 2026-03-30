param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

$env:VCPKG_ROOT = 'C:\Users\heida\vcpkg'
Set-Location $transportDir

cargo test