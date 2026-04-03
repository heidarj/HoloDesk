# Execution Log: Phase 001.3 - Quinn Migration

## Plan File

N/A — corrective migration triggered by persistent MsQuic runtime failures.

## Scope Executed

Replaced the MsQuic-based host QUIC transport with a pure Rust implementation using quinn, rustls, and rcgen. This resolved the persistent `QUIC_STATUS_ALPN_NEG_FAILURE (0x80410007)` that blocked Milestone 1 validation through phases 001, 001.1, and 001.2. The root cause was a version mismatch between the `msquic` Rust crate (v2.5.1-beta FFI bindings) and the vcpkg-installed native MsQuic DLL (v2.4.8).

## Files Changed

- `host/transport/Cargo.toml` — replaced `msquic` with `quinn`, `rustls`, `rcgen`, `tokio`
- `host/transport/Cargo.lock` — regenerated for new dependency tree
- `host/transport/src/lib.rs` — removed MsQuic-specific exports
- `host/transport/src/config.rs` — replaced `WindowsCertificateHash` with `SelfSigned`; removed cert store fields
- `host/transport/src/tls.rs` — complete rewrite: rcgen cert generation + rustls config builders
- `host/transport/src/server.rs` — complete rewrite: async/await quinn server + client (~950 → ~330 lines)
- `host/transport/src/bin/quic_server.rs` — added `#[tokio::main]` async entry point
- `host/transport/src/bin/transport_smoke_client.rs` — added `#[tokio::main]` async entry point
- `host/transport/README.md` — rewritten for quinn
- `docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md` — updated consequences to reference ADR 0003
- `docs/adr/0003-use-quinn-instead-of-msquic.md` — new ADR documenting the migration decision
- `docs/Status.md` — updated to reflect Milestone 1 completion
- `docs/transport-smoke-test.md` — rewritten for quinn (removed vcpkg/cert store prereqs)
- `scripts/host-transport-build.ps1` — removed VCPKG_ROOT
- `scripts/host-transport-test.ps1` — removed VCPKG_ROOT
- `scripts/host-transport-server.ps1` — removed cert SHA-1 / store params
- `scripts/host-transport-client.ps1` — removed VCPKG_ROOT

## What Was Preserved

- `host/transport/src/protocol.rs` — unchanged (JSON framing, ALPN, protocol version)
- `host/transport/src/connection.rs` — unchanged (control-stream state machine)
- `host/transport/tests/codec_roundtrip.rs` — unchanged, all 4 tests pass

## Validation Run

- `cargo build --bins` succeeds with no native dependencies.
- `cargo test` passes all 4 codec roundtrip tests.
- Client-initiated close smoke test: hello → hello_ack → client goodbye → orderly shutdown. Both exit 0.
- Server-initiated close smoke test: hello → hello_ack → server goodbye → orderly shutdown. Both exit 0.

## Validation Result

Milestone 1 is complete for the host side. Both close directions pass. The ALPN negotiation failure is resolved.

## Deviations From Plan

This phase was not in the original plan. It replaced the MsQuic corrective work from phases 001.1 and 001.2 with a complete library migration after determining the MsQuic version mismatch was not practically resolvable within the vcpkg + Rust crate ecosystem.

## Blockers or Missing Answers

- Apple-side `Network.framework` validation is still deferred (no Mac available).
