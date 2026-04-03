# HoloBridge v1 – Project Status

---

## Current Milestone

**Milestone 1 – QUIC / HTTP3 Transport Skeleton** ✅ Complete

Host-side QUIC transport is implemented and validated using quinn (pure Rust). Both client-initiated and server-initiated close scenarios pass on localhost.

---

## Completed Milestones

| Milestone | Description | Completed |
|---|---|---|
| 0 | Repo scaffolding, documentation, and agent setup | ✅ |
| 1 | QUIC transport skeleton (host side) | ✅ |

---

## Latest Changes

- Replaced `msquic` (C FFI) with `quinn` (pure Rust) for the host QUIC transport. See ADR 0003 for rationale.
- Rewrote `host/transport/src/server.rs` from callback-driven MsQuic to async/await quinn (~950 lines → ~330 lines).
- Replaced Windows certificate store configuration with rcgen self-signed certificate generation.
- Removed vcpkg / `VCPKG_ROOT` / native DLL dependency entirely.
- Added tokio async runtime as a dependency.
- Preserved existing control protocol (`protocol.rs`), state machine (`connection.rs`), and codec tests unchanged.

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
- [ ] Apple-side `Network.framework` runtime behavior was not validated (no Mac available).

---

## Known Limitations

- Apple-side transport validation is deferred until a Mac/Xcode environment is available.
- The `client-avp/Transport/` Swift files reference MsQuic ALPN and protocol constants but have not been tested against the quinn-based host.
- Apple bundle ID, team ID, and Sign in with Apple client ID remain unconfigured (required for Milestone 2).

---

## Next Recommended Step

1. Validate the Swift transport files on Apple runtime against the quinn-based host.
2. Begin Milestone 2: Sign in with Apple + Host Authorization.

---

## Blockers

- Apple-side runtime validation requires a Mac/Xcode environment not available in this workspace.
