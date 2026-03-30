# HoloBridge Host Transport

Phase 001.1 corrects the Milestone 1 host transport slice so the checked-in code no longer reports transcript-only success as if a live QUIC round-trip had happened. The current host code stays transport-only: one reliable control stream contract, Windows certificate-store configuration, narrow MsQuic dependency wiring, and explicit failure from the binaries until the live listener/client/stream path is implemented and validated.

## Files

- `Cargo.toml` pins the Rust-side transport dependencies.
- `src/config.rs` defines bind endpoint, ALPN, Windows certificate-store selection, and debug validation configuration.
- `src/protocol.rs` owns the JSON control schema and 4-byte big-endian length framing.
- `src/connection.rs` owns the control-stream state machine for `hello`, `hello_ack`, and `goodbye`.
- `src/tls.rs` keeps Windows thumbprint parsing and client validation decisions isolated from the rest of the transport code.
- `src/server.rs` keeps the MsQuic-facing host boundary narrow and exposes runtime summaries without pretending a transcript is a live runtime.
- `src/bin/quic_server.rs` reports the live configuration it would use and exits failure until the real MsQuic listener path is wired.
- `src/bin/transport_smoke_client.rs` reports the live configuration it would use and exits failure until the real MsQuic client/stream path is wired.

## Configuration

The current binaries read environment variables so the live MsQuic runtime can stay decoupled from higher-level config layers.

- `HOLOBRIDGE_TRANSPORT_BIND`: host bind address. Default `127.0.0.1`.
- `HOLOBRIDGE_TRANSPORT_PORT`: QUIC port. Default `4433`.
- `HOLOBRIDGE_TRANSPORT_ALPN`: application ALPN. Default `holobridge-m1`.
- `VCPKG_ROOT`: required by the `msquic` crate `find` flow so Cargo can locate the native MsQuic installation on Windows.
- `HOLOBRIDGE_TRANSPORT_CERT_SHA1`: required Windows certificate thumbprint for the server certificate in the local certificate store.
- `HOLOBRIDGE_TRANSPORT_CERT_STORE`: Windows certificate store name. Default `MY`.
- `HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE`: when `true`, use `LocalMachine`; otherwise use `CurrentUser`.
- `HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT`: debug-only certificate verification bypass.
- `HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK`: when `true`, model the server-initiated close path.
- `HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE`: when `true`, model the client-initiated close path after `hello_ack`.

## Current Status

- Transcript-driven success output was removed from the default binaries.
- The binaries now fail fast after validating their configuration because the live MsQuic listener/client/stream wiring was not safely implementable in this session.
- The concrete blocker was toolchain visibility: terminal execution returned `ENOPRO`, and no local `msquic` crate source/cache was available for compile-checked API inspection.

## Planned Local Validation Inputs

These are still the inputs the real live smoke path will need once the runtime wiring lands:

- Windows 11 or Windows Server 2022 or newer.
- vcpkg with `ms-quic` installed for `x64-windows` and `VCPKG_ROOT` set.
- A development certificate imported into `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My`.
- A real SHA-1 thumbprint exported through `HOLOBRIDGE_TRANSPORT_CERT_SHA1`.

Use [../../docs/transport-smoke-test.md](../../docs/transport-smoke-test.md) for the exact next-step validation checklist and the environment values that the live binaries will need.

## Validation Notes

This session could validate only editor diagnostics and the corrected configuration surface. It did not build the crate, load native MsQuic, or perform a live QUIC round-trip.