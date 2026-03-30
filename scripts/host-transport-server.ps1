param(
    [string]$Bind = '127.0.0.1',
    [int]$Port = 4433,
    [string]$CertSha1,
    [string]$CertStore = 'MY',
    [bool]$MachineStore = $false,
    [bool]$ServerCloseAfterAck = $false
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($CertSha1)) {
    throw 'CertSha1 is required.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$transportDir = Join-Path $repoRoot 'host\transport'

$env:VCPKG_ROOT = 'C:\Users\heida\vcpkg'
$env:HOLOBRIDGE_TRANSPORT_BIND = $Bind
$env:HOLOBRIDGE_TRANSPORT_PORT = [string]$Port
$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = $CertSha1
$env:HOLOBRIDGE_TRANSPORT_CERT_STORE = $CertStore
$env:HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE = if ($MachineStore) { 'true' } else { 'false' }
$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = if ($ServerCloseAfterAck) { 'true' } else { 'false' }

Set-Location $transportDir

.\target\debug\quic_server.exe