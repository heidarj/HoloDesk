# Execution Log: Milestone 6 – QUIC Video Datagrams + AVP Decode / Display

## Plan File

`docs/Plan.md`

## Scope Executed

Implemented the Milestone 6 host-first video transport path in the Rust workspace and a best-effort matching receive / decode / display path in the canonical visionOS app target. The host now fragments Annex-B H.264 access units into QUIC datagrams after successful auth or resume, and the canonical `client-avp/HoloBridge/HoloBridge` target now has a tunnel-based QUIC receive surface, H.264 datagram reassembly, VideoToolbox decode plumbing, and a Metal-backed flat video surface for connected sessions.

## Key Changes

- Added `host/transport/src/media.rs` with the fixed Milestone 6 datagram header, H.264 packetizer, reassembler, payload-limit negotiation helper, and unit tests for header roundtrips, fragmentation, out-of-order delivery, expiry, and unsupported datagram peers.
- Extended `host/transport` server/client config to carry nested `VideoStreamConfig`, datagram send/receive buffer sizing, and client video-request settings while keeping video disabled by default.
- Enabled QUIC datagrams in the quinn transport config and started a per-connection host video worker after successful auth or resume only when the client hello advertises `video-datagram-h264-v1`.
- Reused the existing DXGI capture + Media Foundation H.264 encode path inside the host video worker and kept `access_unit_id` / reassembly state connection-local so resume starts cleanly on a new QUIC connection.
- Added `video_smoke_client` plus `host-video-build.ps1`, `host-video-test.ps1`, and `host-video-smoke.ps1` for Windows build/test/smoke workflows.
- Added host loopback integration tests that validate:
  - `auth -> media datagram receive -> access-unit reassembly`
  - `abrupt disconnect -> resume -> media restart on the new QUIC connection`
- Updated the canonical visionOS app target under `client-avp/HoloBridge/HoloBridge`:
  - `NetworkFrameworkQuicClient` now uses an `NWMultiplexGroup` / `NWConnectionGroup` tunnel surface, creates a control stream from the tunnel, and exposes video datagrams as an `AsyncThrowingStream`.
  - `SessionManager` now owns a separate video pipeline lifecycle and resets decode/reassembly state on disconnect.
  - Added `Transport/H264VideoDatagram.swift`, `Decode/H264VideoDecoder.swift`, `Session/VideoStreamPipeline.swift`, `Display/VideoRenderer.swift`, and `Display/VideoDisplayView.swift`.
  - `ContentView.swift` now presents a flat video surface with loading/status overlays for connected sessions.

## Validation Run

- `powershell -ExecutionPolicy Bypass -File scripts/host-video-test.ps1`
  - Passed on Windows.
  - Result: 11 transport/media/server tests passed, including the 2 new Milestone 6 loopback video tests and the 6 codec tests.
- `powershell -ExecutionPolicy Bypass -File scripts/host-video-build.ps1`
  - Passed on Windows.
  - Result: built `quic_server`, `video_smoke_client`, and `test_keygen`.
- `powershell -ExecutionPolicy Bypass -File scripts/host-video-smoke.ps1 -DurationSeconds 3`
  - Failed in the current desktop session.
  - Host error: `IDXGIOutput1::DuplicateOutput failed with HRESULT 0x80070005: Access is denied.`
  - Client error: the QUIC connection was closed by the peer with `video-stream-worker-failed (code 1)` before a media datagram arrived.
  - The smoke script now correctly exits non-zero when the client binary fails.
- Apple-side validation:
  - Not run from this Windows desktop.
  - `xcodebuild` and on-device AVP runtime validation remain deferred to a later Mac / AVP pass.

## Result

Milestone 6 implementation is now present in the repo, and the host transport/media behavior is validated through loopback tests and Windows builds. The remaining acceptance work is environment-specific:

- run the host smoke from a Windows console session that has DXGI duplication access
- compile/debug the canonical visionOS app target on Mac hardware / Apple Vision Pro
