param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'

Push-Location $hostDir
try {
    cargo test -p holobridge-encode
}
finally {
    Pop-Location
}
