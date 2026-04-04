param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'

Push-Location $hostDir
try {
    cargo build -p holobridge-encode --bin h264_encode_smoke
}
finally {
    Pop-Location
}
