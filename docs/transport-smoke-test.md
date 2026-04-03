# Transport Smoke Test

Validation checklist for Milestone 1 host transport.

## Shared Contract

- ALPN: `holobridge-m1`
- Protocol version: `1`
- Control capability: `control-stream-v1`
- Message framing: 4-byte big-endian length prefix followed by UTF-8 JSON
- Control messages:
  - `hello`
  - `hello_ack`
  - `goodbye`

## Prerequisites

- Rust and Cargo installed.
- No native dependencies required (quinn is pure Rust).

## Unit Tests

```bash
cd host/transport
cargo test
```

Expected: all 4 codec roundtrip tests pass.

## Client-Initiated Close

Terminal 1 (server):

```bash
cd host/transport
cargo run --bin quic_server
```

Terminal 2 (client):

```bash
cd host/transport
HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT=true cargo run --bin transport_smoke_client
```

Expected log sequence:

- Server: `host transport listener started` → `host transport connection established` → `host transport control stream accepted` → received `hello` → sent `hello_ack` → received `goodbye` → `host transport session complete`
- Client: `transport smoke client connected` → sent `hello` → received `hello_ack` → sent `goodbye` → `transport smoke client session complete`
- Both processes exit with code 0.

## Server-Initiated Close

Terminal 1 (server):

```bash
cd host/transport
HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK=true cargo run --bin quic_server
```

Terminal 2 (client):

```bash
cd host/transport
HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT=true HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE=false cargo run --bin transport_smoke_client
```

Expected log sequence:

- Server: received `hello` → sent `hello_ack` → sent `goodbye` → `host transport session complete`
- Client: sent `hello` → received `hello_ack` → received `goodbye` → `transport smoke client session complete`
- Both processes exit with code 0.

## Failure Signals

Treat any of the following as a failed smoke test:

- ALPN mismatch.
- Protocol version mismatch.
- No `hello_ack` after sending `hello`.
- Connection close without `goodbye` or without an orderly shutdown reason.
- Any log line indicating timeout, transport error, or non-zero exit code.

## Apple Runtime Follow-Up

The Swift transport code under `client-avp/Transport/` still requires local Mac or visionOS validation.

1. Build the transport files into a small Xcode harness or app target.
2. Use the same ALPN, protocol version, and certificate expectations as the Windows host.
3. Connect to the host and verify the `hello`/`hello_ack` round-trip.
4. Repeat both client-initiated and server-initiated close scenarios.
5. Record the actual runtime results in `docs/Status.md`.
