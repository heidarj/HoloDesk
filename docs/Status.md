# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 4 – Display Enumeration + DXGI Capture**

Milestone 3 is complete. The host now creates logical stream sessions after auth, issues 60-minute resume tokens proactively, holds sessions on unexpected disconnect, and resumes them on a later QUIC connection. The visionOS client stores the current session token in memory, attempts one automatic resume on unexpected transport loss, and retries resume before full auth on the next user-triggered connect. Manual end-to-end validation has now succeeded for both Apple identity-token auth and session resume on real hardware.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |
| 1 | QUIC transport skeleton | ✅ |
| 2 | Sign in with Apple + host authorization | ✅ |
| 3 | Stream lifecycle + resume token | ✅ |

---

## Latest Changes

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

- [ ] Native Windows console-session validation is still required for DXGI enumeration and capture acceptance.
- [x] `host/capture/` now exists as a new workspace crate with the planned `CaptureBackend` and `CaptureSession` interfaces plus a `dxgi_capture_smoke` binary.
- [x] `cargo test` in `host/` still passes on macOS with the new capture crate compiled through its non-Windows stub path.
- [x] `cargo build -p holobridge-capture --bin dxgi_capture_smoke` succeeds on macOS via the non-Windows stub implementation.
- [x] `cargo check -p holobridge-capture --target x86_64-pc-windows-msvc` succeeds on macOS after installing the Windows target, confirming the DXGI backend type-checks against the Windows bindings.
- [x] `bash -n scripts/mac-remote-host-capture.sh` succeeds, confirming the remote orchestration script is shell-valid on macOS.
- [x] The repo now includes a remote Windows workflow for `build`, `test`, and `smoke` actions against a native Windows clone.

---

## Known Limitations

- Milestone 4 has not yet been run on the Windows desktop from this workspace, so DXGI enumeration, duplication, and access-loss handling remain unverified until the new remote workflow is exercised against a real console session.
- The capture crate intentionally exposes GPU textures only on Windows. Non-Windows builds compile for workspace health, but all capture entrypoints return `UnsupportedPlatform`.
- Authorization is still effectively first-user bootstrap by default; there is no explicit admin flow yet for reviewing or pre-registering Apple `sub` values.
- Resume state is memory-only on both sides in Milestone 3. If the host process or the app restarts, the user must authenticate again.

---

## Next Recommended Step

1. Push the current branch and run `scripts/mac-remote-host-capture.sh build`, `test`, and `smoke` against the native Windows clone while the Windows machine stays on a real logged-in console session.
2. If the smoke run succeeds on Windows, update the milestone 4 execution log with the native validation results and close out Milestone 4.
3. Begin Milestone 5 by adding the H.264 encoder pipeline that consumes the GPU-resident textures exposed by `holobridge-capture`.

---

## Blockers

- No code blocker is currently known for Milestone 4 planning or scaffolding.
