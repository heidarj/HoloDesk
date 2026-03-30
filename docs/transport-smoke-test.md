# Transport Smoke Test

This document records the current corrective status for Milestone 1 host transport. The transcript-only success path was removed from the checked-in host binaries, but the live MsQuic listener/client/stream runtime was not wired in this session because terminal execution failed with `ENOPRO` and no local `msquic` crate source/cache was available for compile-checked integration.

The settings below are the exact Windows prerequisites and target smoke inputs the next live-runtime pass must use. They are not being claimed as executed from this session.

## Shared Contract

- ALPN: `holobridge-m1`
- Protocol version: `1`
- Control capability: `control-stream-v1`
- Message framing: 4-byte big-endian length prefix followed by UTF-8 JSON
- Control messages:
  - `hello`
  - `hello_ack`
  - `goodbye`

## Windows Prerequisites

- Windows 11 or Windows Server 2022 or newer.
- Rust and Cargo installed on the Windows host validation machine.
- vcpkg installed with `ms-quic` for `x64-windows`, with `VCPKG_ROOT` set so the Rust `msquic` crate can find the native runtime.
- A development certificate imported into `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My`.
- A local terminal that can run `cargo test` and `cargo run` from `host/transport/`.

## Current Checked-In Result

- `host/transport` now uses Windows certificate-store thumbprint configuration instead of the earlier PFX/PEM placeholder defaults.
- The default binaries no longer emit transcript-driven success output.
- No live QUIC runtime validation happened in this session.

## Certificate Thumbprint Check

Confirm the server thumbprint before any live run:

```powershell
Get-ChildItem Cert:\CurrentUser\My | Select-Object Subject, Thumbprint
```

Use `Cert:\LocalMachine\My` instead if `HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE=true`.

## Host Codec and Artifact Check

Run the unit tests before attempting the live smoke path.

```powershell
Set-Location host/transport
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
cargo test
```

Expected result:

- `tests/codec_roundtrip.rs` passes.
- `hello`, `hello_ack`, and `goodbye` framing round-trips succeed.
- The malformed-frame test fails for the expected reason and is reported as a passing negative test.

## Client-Initiated Close

These are the target live inputs once the MsQuic runtime path is wired. The current checked-in binaries will still exit with a runtime-unavailable error after printing their configuration.

Start the host-side server process on the local development endpoint.

```powershell
Set-Location host/transport
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_BIND = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = "<windows-cert-thumbprint>"
$env:HOLOBRIDGE_TRANSPORT_CERT_STORE = "MY"
$env:HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE = "false"
cargo run --bin quic_server
```

Run the host-local smoke client in a second terminal.

```powershell
Set-Location host/transport
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_HOST = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = "true"
$env:HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE = "true"
cargo run --bin transport_smoke_client
```

Expected live log sequence after the runtime is implemented:

- `prepared live host transport configuration`
- `listener started on 127.0.0.1:4433`
- `connection accepted`
- `peer control stream opened`
- `hello received`
- `hello_ack queued`
- `hello_ack received`
- `client goodbye sent`
- `client goodbye received`
- orderly stream and connection shutdown on both sides

Required live sign-off before Milestone 1 is considered complete:

- The server must be confirmed to listen on the configured QUIC endpoint.
- The smoke client must connect over QUIC, not just through an in-process transcript path.
- The logs must show the control message round-trip and clean shutdown without timeout or transport errors.

## Server-Initiated Close

Use the same certificate configuration, then trigger the server-side close path.

```powershell
Set-Location host/transport
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = "<windows-cert-thumbprint>"
$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = "true"
cargo run --bin quic_server
```

```powershell
Set-Location host/transport
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_HOST = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = "true"
cargo run --bin transport_smoke_client
```

Expected live log sequence after the runtime is implemented:

- `listener started on 127.0.0.1:4433`
- `connection accepted`
- `hello received`
- `hello_ack queued`
- `server goodbye sent`
- `server initiated close`
- `orderly shutdown planned`

Required live sign-off before Milestone 1 is considered complete:

- The client must report an orderly remote close instead of a timeout or transport failure.
- The host must release the connection cleanly after sending `goodbye`.

## Failure Signals

Treat any of the following as a failed smoke test:

- Certificate trust failure that is not explained by the configured trust mode.
- ALPN mismatch.
- Protocol version mismatch.
- No `hello_ack` after sending `hello`.
- Connection close without `goodbye` or without an orderly shutdown reason.
- Any log line indicating timeout, leaked connection state, or transport error.

## Apple Runtime Follow-Up

The Swift transport code under `client-avp/Transport/` still requires local Mac or visionOS validation.

1. Build the transport files into a small Xcode harness or app target.
2. Use the same ALPN, protocol version, and certificate expectations as the Windows host.
3. Connect to the Windows host and verify the `hello`/`hello_ack` round-trip.
4. Repeat both client-initiated and server-initiated close scenarios.
5. Record the actual runtime results in `docs/Status.md`.

## Current Session Note

This session verified file completeness, editor diagnostics, the corrected Windows certificate config surface, and the removal of transcript-only success paths. It did not execute the Windows host binaries, the host tests, or the Swift transport on Apple runtime.