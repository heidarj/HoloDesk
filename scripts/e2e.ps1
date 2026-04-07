.\host-video-build.ps1

$env:HOLOBRIDGE_TRANSPORT_BIND = '0.0.0.0'
$env:HOLOBRIDGE_TRANSPORT_PORT = '4433'
$env:HOLOBRIDGE_VIDEO_ENABLED = 'true'
$env:HOLOBRIDGE_VIDEO_FRAME_RATE = '60/1'
$env:HOLOBRIDGE_VIDEO_FIRST_FRAME_TIMEOUT_SECS = '5'
$env:HOLOBRIDGE_AUTH_TEST_MODE = 'false'
$env:HOLOBRIDGE_AUTH_BUNDLE_ID = 'cloud.hr5.HoloBridge'

Push-Location ..\host\transport
..\target\debug\quic_server.exe
Pop-Location
