# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 1 – QUIC / HTTP3 Transport Skeleton**

Corrective host-runtime work is partially applied. Transcript-only success paths were removed, but the live Windows MsQuic host runtime remains incomplete and unvalidated.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |

---

## Latest Changes

- Corrected `host/transport/` so the default binaries no longer report transcript-driven success as if Milestone 1 were live-validated.
- Reworked the host transport configuration toward a Windows MsQuic runtime path: `msquic` now uses the `find` feature, and the host certificate surface now expects a Windows certificate-store SHA-1 thumbprint plus store selection instead of the earlier PFX/PEM placeholder defaults.
- Updated `docs/transport-smoke-test.md` and `host/transport/README.md` to reflect the real Windows prerequisites, the corrected certificate inputs, and the actual incomplete runtime state.
- Added `client-avp/Transport/` with matching ALPN, protocol version, control message framing, a small `TransportClient` abstraction, and a first-pass `Network.framework` QUIC client skeleton.
- Preserved unrelated edits under `.github/agents/` and `.vscode/`.

---

## Validation Results

### Milestone 0

- [x] All required bootstrap files exist
- [x] `docs/streaming-v1.md`, `AGENTS.md`, and both ADRs agree on transport, auth, and codec choices
- [x] `docs/Status.md` (this file) is populated
- [x] Custom agent is defined in `.github/agents/continue-until-blocked.agent.md`
- [x] Repository is ready for autonomous milestone work

### Milestone 1

- [x] `host/transport/` and `client-avp/Transport/` now exist and match the planned Milestone 1 scope.
- [x] Host and client artifacts use the same ALPN (`holobridge-m1`), protocol version (`1`), and control message schema (`hello`, `hello_ack`, `goodbye`).
- [x] Transcript-only success paths were removed from the default host binaries so Milestone 1 is no longer overstated.
- [x] Editor diagnostics report no file-level errors for the updated host transport crate and AVP transport files.
- [x] Manual validation documentation reflects the Windows certificate-store/thumbprint path and the actual incomplete runtime state.
- [ ] Host crate tests were not run in this session because terminal execution failed with `ENOPRO: No file system provider found for resource 'file:///c%3A/Users/heida/source/HoloDesk'`.
- [ ] Windows localhost QUIC listener/client round-trip was not implemented or live-validated in this session.
- [ ] Apple-side `Network.framework` runtime behavior was not validated in this session.

---

## Known Limitations

- Milestone 1 is still not signed off because the host runtime gap is still open and Apple runtime validation has not happened.
- The checked-in host slice now has corrected Windows certificate-store configuration and stricter binaries, but it still lacks a live MsQuic listener/client/stream implementation.
- Tooling access in this session was insufficient for safe binding integration: terminal execution failed with `ENOPRO`, and no local `msquic` crate source/cache was available for compile-checked API inspection.
- Development certificate material, trust configuration, and actual fingerprint values were not available in this session.
- Apple bundle ID, team ID, and Sign in with Apple client ID remain unconfigured and are still required for Milestone 2.

---

## Next Recommended Step

Complete the host runtime corrective subphase on a real Windows toolchain:

1. Restore working terminal/Cargo access in the workspace and confirm the `msquic` crate source and native runtime are discoverable through `VCPKG_ROOT`.
2. Implement the real MsQuic listener, accepted connection, control-stream receive/send path, and shutdown callbacks in `host/transport/`.
3. Run `cargo test` and the Windows smoke path from `docs/transport-smoke-test.md` for both close directions.
4. Update this status file with the actual Windows live-runtime result.
5. After the host runtime is live, validate the Swift transport files on Apple runtime.

---

## Blockers

- Host runtime implementation in this session was blocked by the terminal provider error `ENOPRO: No file system provider found for resource 'file:///c%3A/Users/heida/source/HoloDesk'` and by the lack of a locally inspectable `msquic` crate source/cache for compile-checked binding work.
- Apple-side runtime validation is blocked in this session because no Mac/Xcode or visionOS runtime is available here.
