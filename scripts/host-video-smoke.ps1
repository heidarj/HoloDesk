param(
    [string]$Bind = '127.0.0.1',
    [string]$RemoteHost = '127.0.0.1',
    [int]$Port = 4433,
    [string]$ServerName = 'localhost',
    [string]$DisplayId,
    [int]$DurationSeconds = 5,
    [string]$Output = 'artifacts\video-smoke\holobridge-video-smoke.h264',
    [Nullable[int]]$BitrateBps = $null,
    [string]$FrameRate = '60/1',
    [string]$TestUserSub = 'video-smoke-user',
    [string]$AuthBundleId = 'cloud.hr5.HoloBridge'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'
$transportDir = Join-Path $hostDir 'transport'
$workspaceTargetDir = Join-Path $hostDir 'target'
$serverBinary = Join-Path $workspaceTargetDir 'debug\quic_server.exe'
$clientBinary = Join-Path $workspaceTargetDir 'debug\video_smoke_client.exe'
$keygenBinary = Join-Path $workspaceTargetDir 'debug\test_keygen.exe'
$artifactDir = Join-Path $repoRoot 'artifacts\video-smoke'
$privateKeyPath = Join-Path $artifactDir 'holobridge_test_priv.pem'
$publicKeyPath = Join-Path $artifactDir 'holobridge_test_pub.pem'
$userStorePath = Join-Path $artifactDir 'authorized_users.json'

if ([System.IO.Path]::IsPathRooted($Output)) {
    $outputPath = $Output
}
else {
    $outputPath = Join-Path $repoRoot $Output
}

$outputDirectory = Split-Path -Parent $outputPath
New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

Push-Location $hostDir
try {
    cargo build -p holobridge-transport --bin quic_server --bin video_smoke_client --bin test_keygen
}
finally {
    Pop-Location
}

$env:HOLOBRIDGE_AUTH_TEST_PRIVATE_KEY = $privateKeyPath
$env:HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY = $publicKeyPath

Push-Location $transportDir
try {
    & $keygenBinary
    if ($LASTEXITCODE -ne 0) {
        throw "test_keygen exited with code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$env:HOLOBRIDGE_TRANSPORT_BIND = $Bind
$env:HOLOBRIDGE_TRANSPORT_HOST = $RemoteHost
$env:HOLOBRIDGE_TRANSPORT_PORT = [string]$Port
$env:HOLOBRIDGE_TRANSPORT_SERVER_NAME = $ServerName
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = 'true'
$env:HOLOBRIDGE_AUTH_TEST_MODE = 'true'
$env:HOLOBRIDGE_AUTH_BUNDLE_ID = $AuthBundleId
$env:HOLOBRIDGE_AUTH_USER_STORE = $userStorePath
$env:HOLOBRIDGE_VIDEO_ENABLED = 'true'
$env:HOLOBRIDGE_VIDEO_FRAME_RATE = $FrameRate

if ($DisplayId) {
    $env:HOLOBRIDGE_VIDEO_DISPLAY_ID = $DisplayId
}
else {
    Remove-Item Env:HOLOBRIDGE_VIDEO_DISPLAY_ID -ErrorAction SilentlyContinue
}

if ($BitrateBps.HasValue) {
    $env:HOLOBRIDGE_VIDEO_BITRATE_BPS = [string]$BitrateBps.Value
}
else {
    Remove-Item Env:HOLOBRIDGE_VIDEO_BITRATE_BPS -ErrorAction SilentlyContinue
}

$serverProcess = $null

try {
    $serverProcess = Start-Process -FilePath $serverBinary -WorkingDirectory $transportDir -NoNewWindow -PassThru
    Start-Sleep -Milliseconds 750

    if ($serverProcess.HasExited) {
        throw "quic_server exited before the smoke client started (exit code $($serverProcess.ExitCode))."
    }

    Push-Location $transportDir
    try {
        & $clientBinary `
            --duration-seconds $DurationSeconds `
            --output $outputPath `
            --test-user-sub $TestUserSub
        if ($LASTEXITCODE -ne 0) {
            throw "video_smoke_client exited with code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    if ($serverProcess -and -not $serverProcess.HasExited) {
        Stop-Process -Id $serverProcess.Id
        $serverProcess.WaitForExit()
    }
}

Write-Host "video_smoke_output: $outputPath"
