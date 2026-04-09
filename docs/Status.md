# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 7 – implementation landed, pending on-device acceptance**

Milestone 6 is now validated locally again on macOS, and the Milestone 7 code path has landed across the host, shared Swift package, and canonical visionOS app. The host now accepts hybrid return input (pointer motion by datagram, discrete input by reliable control messages), replays input through a new session-scoped Win32 input layer, and releases stuck buttons / keys on focus loss or disconnect. The Apple client now opens a dedicated stream window, keeps the original window as a utility shell, moves controls into a stream-window ornament, and captures remote input only from the visible video surface. Automated validation is green; the remaining work is real Apple Vision Pro acceptance for ornament interaction, activation-event suppression, and gesture tuning.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |
| 1 | QUIC transport skeleton | ✅ |
| 2 | Sign in with Apple + host authorization | ✅ |
| 3 | Stream lifecycle + resume token | ✅ |
| 4 | Display enumeration + DXGI capture | ✅ |
| 5 | Media Foundation H.264 encode path | ✅ |
| 6 | Video transport + AVP decode / display | ✅ |

---

## Latest Changes

- Added `host/input/` as a new workspace crate with a safe session-scoped input API over Win32 `SendInput`, coordinate clamping, per-session pressed-button / pressed-key tracking, and cleanup on focus loss / disconnect.
- Extended the host and shared Swift protocol layers with `input-pointer-datagram-v1`, media datagram kind `2` for pointer motion, and reliable `pointer_button`, `pointer_wheel`, `keyboard_key`, and `input_focus` control messages.
- Updated the Rust transport server so post-auth control handling now accepts the new input messages, starts an input datagram task when the client advertises support, and feeds both channels into the new host input session.
- Extended `SessionClient` / `TransportClient` with explicit `sendPointerMotion`, `sendPointerButton`, `sendWheel`, `sendKey`, and `setInputFocus` APIs, and added matching codec / loopback coverage in the Swift and Rust test suites.
- Added a dedicated visionOS stream window plus `RemoteInputCaptureView`, keeping the main `ContentView` as a utility shell while moving in-session controls into an ornament attached to the stream window.
- Added client-side remote-input gating based on stream-window visibility and ornament interaction, so the host receives `input_focus` updates and remote input is not generated outside the video surface.
- Fixed a race in the QUIC bridge test mock that could complete the bridge before `start(_:)` registered its callback, making the shared Swift package tests deterministic again.
- Updated `scripts/e2e.ps1` so normal runs keep standard host logging while `.\scripts\e2e.ps1 -Verbose` enables the deep capture/video/encode trace path, panic backtraces, and a timestamped host log under `artifacts/e2e/`.
- Added pointer-aware DXGI capture metadata on the host: capture frames now distinguish image-only, pointer-only, and combined updates, and include pointer position plus optional pointer-shape payloads.
- Added a host-side pointer overlay transport path: pointer position/visibility now travels as a lightweight QUIC datagram, pointer-shape changes travel as reliable control messages, and pointer-only desktop duplication updates no longer have to traverse the H.264 encoder when the client negotiated pointer overlay support.
- Added stage-aware video worker telemetry and a watchdog in `host/transport/` that tracks the current capture/encode/send stage, logs periodic heartbeats, records the last HRESULT/detail, and closes the active stream loudly if the worker wedges instead of silently stopping frame delivery.
- Added documented trace switches for live debugging: `HOLOBRIDGE_CAPTURE_TRACE`, `HOLOBRIDGE_VIDEO_TRACE`, and the pre-existing `HOLOBRIDGE_ENCODE_TRACE`.
- Extended the shared Apple client package and visionOS app pipeline to understand pointer-state datagrams and `pointer_shape` control messages, maintain cursor state separately from decoded video frames, and render a native cursor overlay above the video surface.
- Added focused tests for pointer update classification, pointer datagram codec round-trips, pointer-shape control-message round-trips, session forwarding of `pointer_shape`, and pointer-only host dispatch behavior.
- Extracted the Apple client transport/session/datagram code into a new local Swift package under `client-avp/HoloBridge/Packages/HoloBridgeClient`, with `HoloBridgeClientCore`, `HoloBridgeClientTestAuth`, and a headless `holobridge-client-smoke` executable target.
- Added a shared `SessionClient` actor for `connect -> hello -> auth`, resume-token reuse, background control monitoring, and video datagram forwarding, while keeping Apple auth, decode, rendering, and SwiftUI state management in the visionOS app target.
- Refactored the canonical visionOS app target to consume the shared package instead of the old app-local transport files, and deleted the duplicated app-local `ControlMessage`, `TransportConfiguration`, `TransportClient`, `NetworkFrameworkQuicClient`, and H.264 datagram reassembly sources.
- Promoted the Rust transport crate’s synthetic video path from test-only helpers to runtime configuration via `HOLOBRIDGE_VIDEO_SOURCE=synthetic` and `HOLOBRIDGE_VIDEO_SYNTHETIC_PRESET=transport-loopback-v1`, so auth/resume/media transport can now be exercised on macOS without Windows capture/encode dependencies.
- Added macOS smoke scripts `scripts/client-mac-smoke-local.sh` and `scripts/client-mac-smoke-remote.sh` to build the local Swift smoke executable, generate test keys, start a local Rust host in synthetic-video mode, and run a bounded auth/resume/media smoke pass.
- Added Milestone 6 host transport/media support in `host/transport/`: `VideoStreamConfig`, QUIC datagram buffer configuration in the quinn transport config, the `video-datagram-h264-v1` capability, an H.264 media datagram header/packetizer/reassembler, and a per-connection video worker that starts only after successful auth or resume when the client advertises video support.
- Reused the existing DXGI capture + Media Foundation encoder path inside the new host media worker and kept datagram sequencing connection-local so abrupt disconnects and resume recreate the video worker cleanly on the new QUIC connection.
- Added a Windows-only `video_smoke_client` binary plus Milestone 6 workflow scripts: `host-video-build.ps1`, `host-video-test.ps1`, and `host-video-smoke.ps1`.
- Added host loopback integration coverage for `auth -> media datagram receive -> access-unit reassembly` and `abrupt disconnect -> resume -> media restart on the new QUIC connection`, using synthetic access units under `cfg(test)` so the transport restart behavior is validated without requiring live capture hardware in CI-style tests.
- Refactored the canonical visionOS app target under `client-avp/HoloBridge/HoloBridge` to use a QUIC tunnel client surface, arm video datagram receive before sending `hello`, keep control/auth/resume on a control stream, and hand media datagrams into a separate video pipeline owned by `SessionManager`.
- Added best-effort client-side Milestone 6 pipeline pieces in the canonical app target: a Rust-compatible H.264 datagram reassembler, a VideoToolbox H.264 Annex-B decoder, a Metal `MTKView` renderer that samples decoded NV12 pixel buffers through `CVMetalTextureCache`, and a connected UI that presents a flat video surface with loading / status overlays instead of only connect/disconnect controls.
- Fixed the async MFT event protocol in `MfH264Encoder`: `ProcessOutput` is now always gated on `METransformHaveOutput` events for hardware encoders, `MF_E_NOTACCEPTING` recovery uses a blocking event wait instead of non-blocking polling, and `flush()` uses a dedicated async drain loop that terminates on `METransformDrainComplete`.
- Added `host/encode/` as a new workspace crate with the public `VideoEncoder` surface, `VideoEncoderConfig`, `EncodedAccessUnit`, `EncodeError`, `H264Profile`, and a Windows-only `MfH264Encoder` backed by Media Foundation hardware H.264 MFTs.
- Extended `host/capture/` so Windows capture sessions now expose their underlying `ID3D11Device`, and updated the DXGI capture device creation flags to include D3D11 video support for downstream GPU-only encoding.
- Implemented a Windows-only GPU BGRA -> NV12 conversion stage using the D3D11 video processor and kept the encode path GPU-resident by wrapping NV12 textures with `MFCreateDXGISurfaceBuffer` instead of doing CPU readback.
- Added `h264_encode_smoke` to open capture on the primary or explicit display, encode a bounded run to a raw `.h264` Annex-B stream, and report encoded-frame count, keyframes, total bytes, average encode latency, and effective bitrate.
- Added milestone-5 workflow scripts: `host-encode-build.ps1`, `host-encode-test.ps1`, `host-encode-smoke.ps1`, and `mac-remote-host-encode.sh`, and extended the Windows setup guidance to include the new encode workflow.
- Recorded native Windows milestone-4 validation in the execution log and status: real console-session smoke succeeded, active-motion capture reached the display’s 60 Hz limit, and access loss exits cleanly with `desktop duplication access was lost`.
- Added `host/capture/` as a new workspace crate with a cross-platform `CaptureBackend` / `CaptureSession` API, `DisplayInfo` and `CapturedFrame` types, a non-Windows unsupported stub, and a Windows-only `DxgiCaptureBackend`.
- Implemented Windows DXGI display enumeration and Desktop Duplication session opening in the capture crate, including explicit `DisplayId` selection, primary-display selection, GPU-resident `ID3D11Texture2D` frame acquisition, and automatic `ReleaseFrame` handling.
- Added `dxgi_capture_smoke` as a capture smoke binary that can list displays or open a target display and report frame cadence, timeouts, and final frame dimensions without CPU readback.
- Added milestone-4 workflow scripts: `host-capture-build.ps1`, `host-capture-test.ps1`, `host-capture-smoke.ps1`, and `mac-remote-host-capture.sh` for the push -> SSH -> pull -> build/test/run loop against a native Windows clone.
- Updated the Windows setup guidance and host documentation to reflect the new remote Windows capture workflow and the capture crate’s role in the host workspace.
- Added `host/session/` as a new workspace crate with in-memory `SessionManager`, explicit `Active/Held/Terminated` session states, reconnect counters, 60-minute hold windows, and one-time resume-token rotation.
- Added `host/auth/src/resume_token.rs` plus new `AuthConfig` settings for `HOLOBRIDGE_AUTH_RESUME_TOKEN_TTL` and `HOLOBRIDGE_AUTH_RESUME_TOKEN_SECRET`. Resume tokens are now HMAC-SHA256 signed opaque payloads carrying `session_id`, `expires_at_unix_secs`, and a nonce.
- Extended the Rust control protocol with `resume_session` and `resume_result`, and extended successful `auth_result` payloads with `session_id`, `resume_token`, and `resume_token_ttl_secs`.
- Refactored `host/transport` into a long-running listener that survives reconnects, creates sessions on auth success, holds them on unexpected disconnect, resumes them with a valid token, and terminates them on orderly shutdown.
- Updated the visionOS transport/session client so it stores the current session ID and resume token in memory, adds a `resuming` state, performs one automatic resume attempt on unexpected disconnect, and retries resume before full auth on the next manual `Connect`.
- Added a debug-only `Simulate Network Drop` button in the connected AVP UI so manual end-to-end reconnect validation can force an abrupt QUIC disconnect without sending `goodbye`.

---

## Validation Results

### Milestone 0

- [x] All required bootstrap files exist
- [x] `docs/streaming-v1.md`, `AGENTS.md`, and both ADRs agree on transport, auth, and codec choices
- [x] `docs/Status.md` (this file) is populated
- [x] Custom agent is defined in `.github/agents/continue-until-blocked.agent.md`
- [x] Repository is ready for autonomous milestone work

### Milestone 1

- [x] `host/transport/` and `client-avp/Transport/` exist and match planned scope.
- [x] Host and client artifacts use the same ALPN (`holobridge-m1`), protocol version (`1`), and control message schema.
- [x] `cargo build --bins` succeeds with no native dependencies.
- [x] `cargo test` passes all 4 codec roundtrip tests.
- [x] Client-initiated close: hello → hello_ack → client goodbye → orderly shutdown. Both processes exit 0.
- [x] Server-initiated close: hello → hello_ack → server goodbye → orderly shutdown. Both processes exit 0.
- [x] Apple-side `Network.framework` build surface now compiles via `xcodebuild` for visionOS Simulator.

### Milestone 2

- [x] Host auth tests pass, including real Apple-issued token validation in manual end-to-end testing.
- [x] visionOS app builds successfully with `xcodebuild -project client-avp/HoloBridge/HoloBridge.xcodeproj -scheme HoloBridge -destination 'generic/platform=visionOS Simulator'`.
- [x] The client can select the real Apple auth path at runtime in debug builds instead of being locked to local test tokens.
- [x] The host default audience now matches the checked-in visionOS bundle identifier.
- [x] Live Apple Sign in with Apple on simulator/device HAS been exercised in this workspace.
- [x] Live Apple identity token transmission to the host and Apple JWKS validation HAVE been exercised in this workspace.

### Milestone 3

- [x] `cargo test` now passes all 24 tests across the host workspace (9 auth + 6 session + 3 transport loopback + 6 codec).
- [x] Host sessions are created on successful auth and include proactive 60-minute resume tokens.
- [x] Loopback QUIC tests cover auth -> abrupt disconnect -> resume success.
- [x] Loopback QUIC tests cover resume-token reuse rejection and expired-token rejection.
- [x] The visionOS app still builds successfully with `xcodebuild -project client-avp/HoloBridge/HoloBridge.xcodeproj -scheme HoloBridge -destination 'generic/platform=visionOS Simulator'`.
- [x] Manual end-to-end Apple auth validation succeeded on a real Apple Vision Pro.
- [x] Manual end-to-end session resume validation succeeded on a real Apple Vision Pro; the server logs confirmed `resume_session` handling and `reconnect_count=1`.

### Milestone 4

- [x] Native Windows console-session validation succeeded on the Windows desktop with an attached display and no RDP session in the acceptance path.
- [x] `host/capture/` now exists as a new workspace crate with the planned `CaptureBackend` and `CaptureSession` interfaces plus a `dxgi_capture_smoke` binary.
- [x] `cargo test` in `host/` still passes on macOS with the new capture crate compiled through its non-Windows stub path.
- [x] `cargo build -p holobridge-capture --bin dxgi_capture_smoke` succeeds on macOS via the non-Windows stub implementation.
- [x] `cargo check -p holobridge-capture --target x86_64-pc-windows-msvc` succeeds on macOS after installing the Windows target, confirming the DXGI backend type-checks against the Windows bindings.
- [x] `bash -n scripts/mac-remote-host-capture.sh` succeeds, confirming the remote orchestration script is shell-valid on macOS.
- [x] The repo now includes a remote Windows workflow for `build`, `test`, and `smoke` actions against a native Windows clone.
- [x] Real Windows smoke validation confirmed correct output dimensions (`2560x1440`), reached `182` captured frames over `3` seconds with `16.63 ms` average cadence while video was playing, and cleanly surfaced `desktop duplication access was lost` when the display state changed.

### Milestone 5

- [x] Native Windows hardware validation succeeded on 2026-04-05 on a real console session with a 3840x2160 attached display.
- [x] All 3 hardware tests pass: encoder creation, short-run encode output, and keyframe presence.
- [x] Smoke test produces 221 encoded frames (4 keyframes) over 5 seconds with 1.45 ms average encode latency and a valid 6.3 MB Annex-B H.264 stream.
- [x] Output stream starts with Annex-B start code + SPS NAL (`00 00 00 01 67`), confirming valid H.264 structure. (`ffprobe` was not installed on the test machine; byte-level header validation was performed instead.)
- [x] Async MFT event protocol fix validated: `ProcessOutput` is correctly gated on `METransformHaveOutput`, `MF_E_NOTACCEPTING` uses blocking event wait, and `flush()` terminates on `METransformDrainComplete`.
- [x] `host/encode/` exists as a new workspace crate with the planned encoder API plus a `h264_encode_smoke` binary.
- [x] `cargo test -p holobridge-encode` passes on macOS, including config, GOP, bitrate, and Annex-B helper tests.
- [x] Windows-only hardware tests exist under `host/encode/tests/` for encoder creation, short-run encode output, and keyframe presence.

### Milestone 6

- [x] Real Windows host + Apple Vision Pro validation on 2026-04-07 confirmed that the stream no longer stalls during active desktop interaction; pointer movement stayed synchronized through the new overlay path and the host continued streaming reliably under continuous mouse movement and clicking.
- [x] `scripts/e2e.ps1` now supports both quiet and deep-debug runs from the Windows host workflow, with `-Verbose` enabling capture/video/encode traces plus a timestamped host log artifact.
- [x] `cargo test -q -p holobridge-transport` passes on this Windows host after the pointer-overlay stability work, including the new pointer-shape/control-message and pointer-only dispatch coverage.
- [x] `cargo test -q` passes across the Rust host workspace after the pointer-overlay stability work.
- [ ] `swift test --package-path client-avp/HoloBridge/Packages/HoloBridgeClient` could not be re-run from this Windows desktop on 2026-04-07 because the `swift` toolchain is not installed here.
- [x] `swift test --package-path client-avp/HoloBridge/Packages/HoloBridgeClient` passes on macOS for the extracted shared client package, including control-message codec tests, H.264 datagram reassembly tests, and `SessionClient` auth/resume behavior with mocked transports.
- [x] `xcodebuild` still succeeds for both `generic/platform=visionOS Simulator` and `generic/platform=visionOS` after the visionOS app target was switched to the shared local Swift package.
- [x] The repo now includes a local macOS smoke loop: `scripts/client-mac-smoke-local.sh` builds the Rust host, generates test keys, starts a synthetic-video host session, and launches the new `holobridge-client-smoke` executable.
- [x] `cargo test -p holobridge-transport` passes on Windows after the Milestone 6 transport/media work, including 2 new loopback integration tests for video datagram startup and video restart after resume.
- [x] `scripts/host-video-build.ps1` succeeds and builds `quic_server`, `video_smoke_client`, and `test_keygen`.
- [x] The host transport now defaults video off, preserving Milestone 1-5 smoke behavior unless `HOLOBRIDGE_VIDEO_ENABLED=true` is set.
- [x] Host loopback validation covers header encode/decode, fragmentation/reassembly, out-of-order fragments, incomplete-frame expiry, auth -> video datagram receive, and resume-triggered media restart on a new QUIC connection.
- [ ] `scripts/client-mac-smoke-local.sh` currently fails at the Apple QUIC client transport stage on macOS before `hello`/`hello_ack` completes, even against the synthetic-video Rust host. The latest observed error after extracting the shared client core is `QUIC connection failed: POSIXErrorCode(rawValue: 50): Network is down`.
- [ ] Native Windows `scripts/host-video-smoke.ps1` did not complete successfully in the current desktop session on 2026-04-05: `IDXGIOutput1::DuplicateOutput` failed with `0x80070005 (Access is denied)`, causing the host to close the QUIC connection before the smoke client received video datagrams.
- [x] `xcodebuild -project client-avp/HoloBridge/HoloBridge.xcodeproj -scheme HoloBridge -destination 'generic/platform=visionOS Simulator' build` was rerun on macOS on 2026-04-09 and still succeeds after the dedicated stream-window / input-return-path changes.

### Milestone 7

- [x] `host/input/` now exists as a new workspace crate with unit coverage for input codec handling, coordinate clamping, focus transitions, and stuck-input cleanup.
- [x] `cargo test -q -p holobridge-input` passes on macOS.
- [x] `cargo test -q -p holobridge-transport` passes on macOS, including the new loopback input path coverage.
- [x] `cargo test -q` passes across the Rust host workspace after the Milestone 7 input work.
- [x] `swift test --package-path client-avp/HoloBridge/Packages/HoloBridgeClient --scratch-path /tmp/HoloBridgeClientTestsAll` passes on macOS, including the new input codec tests and the repaired `NetworkFrameworkQuicClientTests`.
- [x] `xcodebuild -project client-avp/HoloBridge/HoloBridge.xcodeproj -scheme HoloBridge -destination 'generic/platform=visionOS Simulator' build` passes on macOS with the new utility-shell + stream-window design.
- [x] The canonical visionOS app now keeps connect / status / disconnect UI in the original shell window and opens a dedicated stream window for the video surface.
- [x] The host now accepts pointer motion over QUIC datagrams and button / wheel / keyboard / focus updates over the reliable control stream after auth or resume.
- [ ] Real Apple Vision Pro acceptance for Milestone 7 is still pending: separate-window UX, ornament-safe interaction, first-activation suppression, hardware-key forwarding, and device-measured input latency still need on-device verification.

---

## Known Limitations

- Masked-color Windows cursor shapes are currently approximated as RGBA images on the host. Typical color and monochrome cursors are handled, but XOR-style masked-color cursors may not render perfectly yet.
- The host watchdog now fails loudly when the worker stalls, but the underlying Media Foundation / D3D11 calls are still in-process. If Windows wedges inside a driver path that ignores flush/close, a full process restart may still be required.
- The capture crate intentionally exposes GPU textures only on Windows. Non-Windows builds compile for workspace health, but all capture entrypoints return `UnsupportedPlatform`.
- The Media Foundation backend currently selects the first compatible hardware H.264 MFT. There is no vendor-specific encoder selection or capability ranking yet.
- Authorization is still effectively first-user bootstrap by default; there is no explicit admin flow yet for reviewing or pre-registering Apple `sub` values.
- Resume state is memory-only on both sides in Milestone 3. If the host process or the app restarts, the user must authenticate again.
- The new Apple shared client package is structurally in place and tested with mocked transports, but the real macOS/visionOS `Network.framework` QUIC media path is not yet accepted. The local smoke executable currently fails during real QUIC connection establishment against the Rust host, before auth or media delivery begins.
- The dedicated stream window and return-input path now build and test on macOS, but the Milestone 7 interaction model has not yet been exercised on a real Apple Vision Pro. Ornament-safe interaction and first-activation suppression still need device validation.
- Milestone 7 intentionally supports only the `absolute surface` pointer policy. Relative trackpad / explicit capture modes are deferred to Milestone 7.1.
- Software keyboard / text-entry forwarding is not part of Milestone 7. Only hardware-key replay is implemented.
- Scroll replay currently uses a best-effort two-finger pan mapping on the client side and may need device tuning.
- The current Windows desktop session used for Milestone 6 smoke validation did not grant DXGI Desktop Duplication access (`0x80070005`). A real local console session with duplication access is still required for native host video smoke acceptance.

---

## Next Recommended Step

Run the Milestone 7 manual acceptance pass on a real Apple Vision Pro against a Windows host: verify separate-window presentation, ornament-safe interaction, first-activation suppression, drag / wheel / hardware-key replay, and sequence-number-based latency measurements. If that pass is clean, close Milestone 7 and move to Milestone 8 audio; if device testing exposes UX gaps, carve the follow-up work into Milestone 7.1 for alternate pointer policies, text entry, and gesture tuning.

---

## Blockers

None.
