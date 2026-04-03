# HoloBridge Host Transport

QUIC transport layer for the HoloBridge host, built on **quinn** (pure Rust QUIC) with **rustls** for TLS and **rcgen** for self-signed certificate generation.

## Architecture

The transport uses quinn's async/await API with tokio. The server generates a self-signed certificate at startup, eliminating the need for Windows certificate store configuration or external native dependencies.

## Files

- `Cargo.toml` pins the Rust-side transport dependencies (quinn, rustls, rcgen, tokio).
- `src/config.rs` defines bind endpoint, ALPN, and debug validation configuration.
- `src/protocol.rs` owns the JSON control schema and 4-byte big-endian length framing.
- `src/connection.rs` owns the control-stream state machine for `hello`, `hello_ack`, and `goodbye`.
- `src/tls.rs` generates self-signed certificates via rcgen and builds rustls server/client configs.
- `src/server.rs` implements the async QUIC server and smoke client using quinn.
- `src/bin/quic_server.rs` async server binary entry point.
- `src/bin/transport_smoke_client.rs` async smoke client binary entry point.

## Configuration

The binaries read environment variables for configuration.

- `HOLOBRIDGE_TRANSPORT_BIND`: host bind address. Default `127.0.0.1`.
- `HOLOBRIDGE_TRANSPORT_PORT`: QUIC port. Default `4433`.
- `HOLOBRIDGE_TRANSPORT_ALPN`: application ALPN. Default `holobridge-m1`.
- `HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT`: debug-only certificate verification bypass. Default `false`.
- `HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK`: when `true`, model the server-initiated close path.
- `HOLOBRIDGE_TRANSPORT_SERVER_WAIT_TIMEOUT_SECS`: host-side wait timeout for incoming connections and control stream open. Default `60`. Set to `0` to disable.
- `HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE`: when `true`, model the client-initiated close path after `hello_ack`.
- `HOLOBRIDGE_TRANSPORT_HOST`: client target host. Default `127.0.0.1`.
- `HOLOBRIDGE_TRANSPORT_SERVER_NAME`: TLS server name for the client. Default `localhost`.

## Quick Start

```bash
# Build
cargo build --bins

# Run tests
cargo test

# Server (terminal 1)
cargo run --bin quic_server

# Client (terminal 2)
HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT=true cargo run --bin transport_smoke_client
```

No vcpkg, no native dependencies, no Windows certificate store setup required.

## Current Status

Milestone 1 is complete. Both client-initiated and server-initiated close scenarios pass on localhost with orderly `hello` -> `hello_ack` -> `goodbye` round-trips.
