param(
    [string]$Host = '127.0.0.1',
    [int]$Port = 4433,
    [bool]$AllowInsecureCert = $true,
    [bool]$ClientSendGoodbye = $true,
    [string]$ServerName = 'localhost'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

$env:HOLOBRIDGE_TRANSPORT_HOST = $Host
$env:HOLOBRIDGE_TRANSPORT_PORT = [string]$Port
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = if ($AllowInsecureCert) { 'true' } else { 'false' }
$env:HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE = if ($ClientSendGoodbye) { 'true' } else { 'false' }
$env:HOLOBRIDGE_TRANSPORT_SERVER_NAME = $ServerName

Set-Location $transportDir

.\target\debug\transport_smoke_client.exe
