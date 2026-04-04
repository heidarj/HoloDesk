param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$captureDir = Join-Path $repoRoot 'host\capture'

Push-Location $captureDir
try {
    cargo build --bins
}
finally {
    Pop-Location
}
