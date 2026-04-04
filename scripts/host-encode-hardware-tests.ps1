param(
    [string]$TestName
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$hostDir = Join-Path $repoRoot 'host'

Push-Location $hostDir
try {
    $arguments = @(
        'test',
        '-p', 'holobridge-encode',
        '--test', 'windows_hardware'
    )

    if ($TestName) {
        $arguments += $TestName
    }

    $arguments += '--'
    $arguments += '--ignored'
    $arguments += '--nocapture'

    & cargo @arguments
}
finally {
    Pop-Location
}
