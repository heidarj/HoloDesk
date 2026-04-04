param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

Push-Location $transportDir
try {
    cargo build --bins
}
finally {
    Pop-Location
}
