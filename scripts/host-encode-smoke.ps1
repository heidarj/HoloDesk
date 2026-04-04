param(
    [string]$DisplayId,
    [int]$DurationSeconds = 5,
    [int]$TimeoutMs = 16,
    [int]$FirstFrameTimeoutSeconds = 2,
    [string]$Output = 'holobridge-smoke.h264',
    [Nullable[int]]$BitrateBps = $null,
    [string]$FrameRate = '60/1'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'
$workspaceTargetDir = Join-Path $hostDir 'target'
$binaryPath = Join-Path $workspaceTargetDir 'debug\h264_encode_smoke.exe'

Push-Location $hostDir
try {
    cargo build -p holobridge-encode --bin h264_encode_smoke

    $arguments = @(
        '--duration-seconds', [string]$DurationSeconds,
        '--timeout-ms', [string]$TimeoutMs,
        '--first-frame-timeout-seconds', [string]$FirstFrameTimeoutSeconds,
        '--output', $Output,
        '--frame-rate', $FrameRate
    )

    if ($DisplayId) {
        $arguments += '--display-id'
        $arguments += $DisplayId
    }

    if ($BitrateBps.HasValue) {
        $arguments += '--bitrate-bps'
        $arguments += [string]$BitrateBps.Value
    }

    & $binaryPath @arguments
}
finally {
    Pop-Location
}
