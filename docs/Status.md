# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 2 – Sign in with Apple + Host Authorization** In Progress

The host-side Apple JWT validator and the client-side auth handshake are implemented. The client can now switch between local test JWTs and the real Sign in with Apple flow from the visionOS app, and the host default `aud` now matches the checked-in bundle ID. The remaining Milestone 2 gap is a live Apple-issued token run on simulator/device all the way through Apple JWKS validation and first-user authorization.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |
| 1 | QUIC transport skeleton | ✅ |

---

## Latest Changes

- `client-avp/HoloBridge/HoloBridge/Auth/AuthService.swift` now uses a presentation anchor and a real `ASAuthorizationController` flow so the app can request an Apple identity token instead of only using locally-signed test JWTs.
- `client-avp/HoloBridge/HoloBridge/Session/SessionManager.swift` now supports runtime auth-mode selection (`apple` vs `test`) instead of hard-wiring test auth in debug builds.
- `client-avp/HoloBridge/HoloBridge/ContentView.swift` exposes the auth mode in debug builds and labels the Apple path as `Sign In and Connect`.
- `host/auth/src/config.rs` now defaults the expected Apple `aud` to the checked-in bundle ID `cloud.hr5.HoloBridge`, which matches the visionOS project.
- `host/transport/src/bin/auth_smoke_client.rs` now uses the same bundle ID default so the local test-token flow still aligns with host validation.
- `client-avp/HoloBridge/HoloBridge.xcodeproj/project.pbxproj` now includes the missing local Swift package reference for `Packages/RealityKitContent`, which was required to make `xcodebuild` resolve and build the project from this workspace.

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

- [x] Host auth tests pass: `cargo test` reports 10/10 passing tests (6 auth + 4 transport codec).
- [x] visionOS app builds successfully with `xcodebuild -project client-avp/HoloBridge/HoloBridge.xcodeproj -scheme HoloBridge -destination 'generic/platform=visionOS Simulator'`.
- [x] The client can select the real Apple auth path at runtime in debug builds instead of being locked to local test tokens.
- [x] The host default audience now matches the checked-in visionOS bundle identifier.
- [ ] Live Apple Sign in with Apple on simulator/device has not yet been exercised in this workspace.
- [ ] Live Apple identity token transmission to the host and Apple JWKS validation have not yet been exercised in this workspace.

---

## Known Limitations

- The real Apple auth path still needs a signed-in Apple simulator/device run to confirm `identityToken` issuance end-to-end.
- Authorization is still effectively first-user bootstrap by default; there is no explicit admin flow yet for reviewing or pre-registering Apple `sub` values.
- The visionOS transport client still has pre-existing concurrency/deprecation warnings in `NetworkFrameworkQuicClient.swift`; they do not block Milestone 2 auth validation but should be cleaned up before tightening Swift 6 checks.

---

## Next Recommended Step

1. Run the app in `Apple` auth mode on a signed-in visionOS simulator or device.
2. Start the host with `HOLOBRIDGE_AUTH_TEST_MODE=0` and `HOLOBRIDGE_AUTH_BUNDLE_ID=cloud.hr5.HoloBridge` (or your actual bundle ID if you change the project).
3. Verify `Hello -> HelloAck -> Authenticate -> AuthResult success` using a real Apple identity token.
4. Capture the first real Apple `sub` in `host/authorized_users.json`, then decide whether to keep bootstrap enabled or move to explicit authorization for subsequent users.

---

## Blockers

- No code blocker is currently known for the Milestone 2 auth slice.
- Full Milestone 2 completion still depends on live Apple account/device state and Apple network reachability during runtime testing.
