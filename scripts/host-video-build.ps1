param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'

Push-Location $hostDir
try {
    cargo build -p holobridge-transport --bin quic_server --bin video_smoke_client --bin test_keygen
}
finally {
    Pop-Location
}
