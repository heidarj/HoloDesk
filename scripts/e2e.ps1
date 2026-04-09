[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'
$transportDir = Join-Path $hostDir 'transport'
$serverBinary = Join-Path $hostDir 'target\debug\quic_server.exe'
$buildScript = Join-Path $PSScriptRoot 'host-video-build.ps1'
$logDir = Join-Path $repoRoot 'artifacts\e2e'
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$logPath = Join-Path $logDir "e2e-host-$timestamp.log"

& $buildScript

$env:HOLOBRIDGE_TRANSPORT_BIND = '0.0.0.0'
$env:HOLOBRIDGE_TRANSPORT_PORT = '4433'
$env:HOLOBRIDGE_VIDEO_ENABLED = 'true'
$env:HOLOBRIDGE_VIDEO_FRAME_RATE = '60/1'
$env:HOLOBRIDGE_VIDEO_FIRST_FRAME_TIMEOUT_SECS = '5'
$env:HOLOBRIDGE_AUTH_TEST_MODE = 'false'
$env:HOLOBRIDGE_AUTH_BUNDLE_ID = 'cloud.hr5.HoloBridge'
$env:RUST_BACKTRACE = '1'

New-Item -ItemType Directory -Force -Path $logDir | Out-Null

Write-Host "Writing host log to: $logPath"

if ($VerbosePreference -eq 'Continue') {
    $env:HOLOBRIDGE_CAPTURE_TRACE = '1'
    $env:HOLOBRIDGE_VIDEO_TRACE = '1'
    $env:HOLOBRIDGE_ENCODE_TRACE = '1'
    $env:RUST_LOG = 'debug'
    Write-Host 'Enabled host traces:'
    Write-Host '  HOLOBRIDGE_CAPTURE_TRACE=1'
    Write-Host '  HOLOBRIDGE_VIDEO_TRACE=1'
    Write-Host '  HOLOBRIDGE_ENCODE_TRACE=1'
    Write-Host '  RUST_LOG=debug'
    Write-Host '  RUST_BACKTRACE=1'
} else {
    Remove-Item Env:HOLOBRIDGE_CAPTURE_TRACE -ErrorAction SilentlyContinue
    Remove-Item Env:HOLOBRIDGE_VIDEO_TRACE -ErrorAction SilentlyContinue
    Remove-Item Env:HOLOBRIDGE_ENCODE_TRACE -ErrorAction SilentlyContinue
    Remove-Item Env:RUST_LOG -ErrorAction SilentlyContinue
    Write-Host 'Running with normal host logging. Use -Verbose to enable deep capture/video/encode traces.'
}

Push-Location $transportDir
try {
    & $serverBinary 2>&1 | Tee-Object -FilePath $logPath
    $exitCode = $LASTEXITCODE
    Write-Host "quic_server exit code: $exitCode"
    if ($exitCode -ne 0) {
        throw "quic_server exited with code $exitCode. See $logPath"
    }
}
finally {
    Pop-Location
}
