# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 5 – Media Foundation H.264 Encode Path**

Milestones 1 through 4 are complete. The host now enumerates Windows displays, opens DXGI Desktop Duplication in a real Windows console session, acquires GPU-resident desktop textures at the active display cadence, and surfaces `DXGI_ERROR_ACCESS_LOST` cleanly on display-state changes. Milestone 5 is now implemented in code with a new Media Foundation H.264 encoder crate and remote Windows encode workflow, but its Windows smoke run and `ffprobe` validation are still pending.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |
| 1 | QUIC transport skeleton | ✅ |
| 2 | Sign in with Apple + host authorization | ✅ |
| 3 | Stream lifecycle + resume token | ✅ |
| 4 | Display enumeration + DXGI capture | ✅ |

---

## Latest Changes

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

- [ ] Native Windows H.264 smoke validation and `ffprobe -f h264` validation are still required for milestone acceptance.
- [x] `host/encode/` now exists as a new workspace crate with the planned encoder API plus a `h264_encode_smoke` binary.
- [x] `cargo test` in `host/` still passes on macOS with the new encode crate compiled through its non-Windows stub path.
- [x] `cargo test -p holobridge-encode` passes on macOS, including config, GOP, bitrate, and Annex-B helper tests.
- [x] `cargo build -p holobridge-encode --bin h264_encode_smoke` succeeds on macOS via the unsupported-platform stub path.
- [x] `cargo check -p holobridge-encode --target x86_64-pc-windows-msvc` succeeds on macOS, confirming the Media Foundation / D3D11 backend type-checks against the Windows bindings.
- [x] `bash -n scripts/mac-remote-host-encode.sh` succeeds, confirming the remote encode orchestration script is shell-valid on macOS.
- [x] Windows-only ignored hardware tests now exist under `host/encode/tests/` for encoder creation, short-run encode output, and keyframe presence.

---

## Known Limitations

- The capture crate intentionally exposes GPU textures only on Windows. Non-Windows builds compile for workspace health, but all capture entrypoints return `UnsupportedPlatform`.
- Milestone 5 has not yet been run on the Windows desktop from this workspace, so hardware H.264 encoder availability, real encode throughput, `.h264` output validity, and `ffprobe` acceptance are not yet recorded.
- The Media Foundation backend currently selects the first compatible hardware H.264 MFT. There is no vendor-specific encoder selection or capability ranking yet.
- Authorization is still effectively first-user bootstrap by default; there is no explicit admin flow yet for reviewing or pre-registering Apple `sub` values.
- Resume state is memory-only on both sides in Milestone 3. If the host process or the app restarts, the user must authenticate again.

---

## Next Recommended Step

1. Push the current branch and run `scripts/mac-remote-host-encode.sh build`, `test`, and `smoke` against the native Windows clone while the Windows machine stays on a real logged-in console session with an attached display.
2. Validate the generated `.h264` output with `ffprobe -f h264 <output.h264>` and confirm encoded access units, keyframes, bitrate, and latency are reasonable while the desktop is visibly changing.
3. If the encode smoke run succeeds on Windows, add the native validation results to the milestone 5 execution log and move on to milestone 6 transport of encoded access units.

---

## Blockers

- No code blocker is currently known for Milestone 5 implementation. The remaining work is native Windows validation.
