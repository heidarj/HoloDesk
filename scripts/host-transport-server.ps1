param(
    [string]$Bind = '127.0.0.1',
    [int]$Port = 4433,
    [bool]$ServerCloseAfterAck = $false
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

$env:HOLOBRIDGE_TRANSPORT_BIND = $Bind
$env:HOLOBRIDGE_TRANSPORT_PORT = [string]$Port
$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = if ($ServerCloseAfterAck) { 'true' } else { 'false' }

Set-Location $transportDir

.\target\debug\quic_server.exe
