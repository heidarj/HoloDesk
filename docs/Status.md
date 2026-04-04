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

---

## Known Limitations

- Authorization is still effectively first-user bootstrap by default; there is no explicit admin flow yet for reviewing or pre-registering Apple `sub` values.
- Resume state is memory-only on both sides in Milestone 3. If the host process or the app restarts, the user must authenticate again.
- The visionOS transport client still has pre-existing concurrency/deprecation warnings in `NetworkFrameworkQuicClient.swift`; they do not block Milestone 3 validation but should be cleaned up before tightening Swift 6 checks.

---

## Next Recommended Step

1. Start Milestone 4 by scaffolding `host/capture/` as a Rust crate in the host workspace.
2. Implement display enumeration for the primary DXGI path and define the `ICaptureBackend`-style interface boundary for future backends.
3. Add a minimal validation path that enumerates displays and proves frame acquisition can be opened for the target display.

---

## Blockers

- No code blocker is currently known for Milestone 4 planning or scaffolding.
