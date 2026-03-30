# Phase Plan: Phase 001.1 - MsQuic Host Runtime

## Goal

Complete only the missing host-side runtime portion of Milestone 1 by replacing the current transcript harness with a real Windows MsQuic listener, connection, and control-stream path that can accept a live QUIC connection, exchange one control message round-trip, and close cleanly. Keep the existing JSON control contract, leave the AVP transport scaffolding as-is unless a host-side contract mismatch is discovered, and include the directly required status and execution-log corrections so Milestone 1 is not overstated.

## Acceptance Criteria

- `host/transport/src/server.rs` starts a real MsQuic listener on Windows instead of only returning a planned callback sequence.
- `host/transport/src/bin/quic_server.rs` runs a live host process that listens, accepts a QUIC connection, receives `hello` on a real bidirectional stream, sends `hello_ack`, and closes cleanly.
- `host/transport/src/bin/transport_smoke_client.rs` becomes a live MsQuic client that connects to the server, opens a control stream, sends `hello`, receives `hello_ack`, and exercises both client-initiated and server-initiated close paths.
- Default success output for the host binaries comes only from live transport execution. Transcript-only success paths are removed or demoted to test helpers so they cannot be mistaken for Milestone 1 completion.
- The host runtime uses a concrete MsQuic credential configuration that actually works on Windows for local development, with the unsafe boundary kept narrow and documented in code structure.
- Validation artifacts are updated so a local Windows developer can run the exact smoke path outside this browser session.
- `docs/Status.md` is updated to reflect the true state after implementation and local validation, and a corrective execution log is added for this subphase.

## Relevant Existing Context

- `docs/Plan.md` defines Milestone 1 as a minimal QUIC/HTTP3 transport skeleton that must listen, accept a connection, exchange one control message round-trip, and close cleanly.
- `docs/streaming-v1.md`, ADR 0001, and `host.instructions.md` require QUIC for transport, reliable streams for control traffic, a Rust-first Windows host, and narrow low-level boundaries.
- `docs/plans/phase-001-quic-http3-transport-skeleton.plan.md` scoped Milestone 1 correctly, but the corresponding execution stopped at a transcript harness instead of a live runtime.
- `docs/execution-logs/phase-001-quic-http3-transport-skeleton.exec.md` explicitly records that the checked-in host slice is not a live on-wire MsQuic round-trip.
- The current host crate already contains reusable pieces:
  - `src/protocol.rs` has the control message schema and framing.
  - `src/connection.rs` has message-order validation for `hello`, `hello_ack`, and `goodbye`.
  - `src/config.rs` already centralizes environment-driven transport config.
- The current missing pieces are concentrated in:
  - `src/server.rs`, which only produces `ListenerPlan` and `SmokeRoundTrip` transcripts.
  - `src/bin/quic_server.rs`, which only prints planned callbacks.
  - `src/bin/transport_smoke_client.rs`, which only replays an in-process transcript.
- `docs/Status.md` currently describes Milestone 1 as artifact-complete but runtime-pending. That was accurate for the previous session, but once the host runtime subphase is implemented it must be updated with actual live validation results rather than remaining ambiguous.

## Verified Findings

- The checked-in crate already pins `msquic = =2.4.0`, but it currently uses the crate only as a placeholder import. The live listener, connection, and stream wrapper APIs still need to be wired in.
- The official MsQuic Rust layer already exposes the exact objects needed for this subphase: `Registration`, `Configuration`, `Listener`, `Connection`, `Stream`, `ListenerEvent`, `ConnectionEvent`, `StreamEvent`, `Settings`, `CredentialConfig`, and concrete `Credential` types.
- Official MsQuic docs state that `ListenerStart` may begin delivering callbacks before the call returns. On `NEW_CONNECTION`, the server must set the connection callback and call `ConnectionSetConfiguration` before returning, or the handshake will stall and time out.
- Official MsQuic docs also require the app to set the stream callback immediately when `PEER_STREAM_STARTED` is delivered, before returning from the connection callback.
- The upstream Rust binding warns that closing or dropping a handle from inside its currently executing callback is unsafe because the callback context is still live. Handle ownership must therefore be transferred out of the callback and reclaimed on shutdown-complete events or after the callback returns.
- The upstream Rust binding requires send buffers to remain valid until `SendComplete` is delivered. Any control-stream payload passed to `Stream::send` must therefore be heap-owned and explicitly reclaimed in the send callback.
- Official MsQuic build docs mark Rust support as experimental and not officially supported. That is acceptable for Milestone 1, but only if the MsQuic-specific lifetime and ownership code stays tightly isolated.
- Official MsQuic Windows support uses Schannel by default and requires Windows 11 or Windows Server 2022 or newer for TLS 1.3 support.
- The upstream Rust config layer marks `CertificateHash` and `CertificateHashStore` as the Windows Schannel-oriented server credential types, while `CertificateFile`, `CertificateFileProtected`, and `CertificatePkcs12` are quictls-oriented. That means the current repo's PFX/PEM-first host config is not the right default for a Windows Schannel runtime path.
- The upstream Rust crate's `find` feature is designed to locate a preinstalled MsQuic on Windows via `VCPKG_ROOT\installed\x64-windows\{bin\msquic.dll, lib\msquic.lib}` and copy the native DLL/lib into Cargo output directories. That is the narrowest local dependency path for this corrective subphase.
- The upstream Rust tests already demonstrate the exact event flow needed here: listener opens, new connection installs a connection callback and configuration, client connects, client opens a stream, server handles `PeerStreamStarted`, and handles are reclaimed on shutdown-complete callbacks.

## Recommended Technical Approach

Implement the corrective subphase as a real Windows-only MsQuic runtime path that preserves the existing control protocol and reuses as much of the current framing/state logic as possible.

- Dependency choice:
  - Keep the existing `msquic` crate version pin initially.
  - Enable the upstream `find` feature on Windows so local builds resolve a preinstalled MsQuic via vcpkg instead of adding a source-build pipeline in this phase.
  - Do not switch libraries. If the Rust binding proves unusable after one focused implementation attempt, the fallback is a narrow direct FFI wrapper over MsQuic, not a library swap.
- Runtime boundary:
  - Keep `protocol.rs` as the framing and schema layer.
  - Keep `connection.rs` as the application-level control-stream state machine, but extend it to support live stream receive/send state instead of transcript-only bookkeeping.
  - Move all direct MsQuic handle ownership, callback installation, send-buffer lifetime management, and shutdown cleanup into `server.rs`, `connection.rs`, and `tls.rs` only.
  - Keep `lib.rs` re-exports small and remove transcript-centric public types from the default API surface if they would confuse implementers or users.
- Host listener and connection lifecycle:
  - Build a real `TransportServer` that owns a MsQuic `Registration`, `Configuration`, and `Listener` plus shared runtime state for one active smoke connection.
  - Use `Settings` appropriate for a single-control-stream milestone slice: one bidirectional stream, no datagram work, finite idle timeout, and no speculative resume/datagram settings.
  - In `ListenerEvent::NewConnection`, immediately install the connection callback, call `set_configuration`, and transfer the accepted connection handle out of the callback safely so it lives until shutdown completion.
  - In `ConnectionEvent::PeerStreamStarted`, immediately install the stream callback and begin control message processing.
  - On `ConnectionEvent::ShutdownComplete` and `StreamEvent::ShutdownComplete`, reclaim and close the corresponding owned handles outside the hazardous callback-drop pattern.
- Control-stream behavior:
  - Keep the current 4-byte big-endian length prefix plus UTF-8 JSON payload framing.
  - Keep the current message set and protocol version unchanged: `hello`, `hello_ack`, `goodbye`, version `1`, ALPN `holobridge-m1`.
  - Reuse `FrameAccumulator` to handle split receives and reuse `ControlConnection` to validate message ordering.
  - For sent frames, allocate the encoded payload and `BufferRef` container on the heap and reclaim them in `StreamEvent::SendComplete`.
- Certificate and validation handling:
  - Rework server certificate config so the Windows runtime path uses a certificate already imported into the Windows certificate store, addressed by thumbprint/hash, rather than assuming PFX/PEM file loading.
  - Prefer `Credential::CertificateHash` or `Credential::CertificateHashStore` for the server, with store defaults aligned to local dev on Windows.
  - Keep client validation for the host-local smoke client intentionally narrow in this phase:
    - default debug smoke path: `NO_CERTIFICATE_VALIDATION`, clearly documented as local-development-only.
    - optional stricter local path: trust the dev certificate in the Windows trust store and omit the insecure flag.
  - Defer SHA-256 pinning and custom certificate-validation callbacks unless they become necessary to get the live smoke path working. They are not required for Milestone 1 acceptance and would widen the callback surface unnecessarily.
- Executable behavior:
  - `quic_server` should run as a live process, not a plan printer.
  - `transport_smoke_client` should run as a live client, not a transcript driver.
  - Transcript-only code can remain only as a test helper if it still adds value for unit coverage; it must not be the default runtime path.
- Documentation and reporting:
  - Update the smoke-test doc and host transport README to describe real prerequisites: Windows version, vcpkg-provided MsQuic, certificate-store thumbprint, and exact commands.
  - Add a new corrective execution log for the subphase and update `docs/Status.md` with the actual local validation result. If local validation still has not been run by the implementer, Milestone 1 must remain explicitly incomplete.

## Likely Files and Modules to Change

- `host/transport/Cargo.toml`
- `host/transport/src/lib.rs`
- `host/transport/src/config.rs`
- `host/transport/src/connection.rs`
- `host/transport/src/server.rs`
- `host/transport/src/tls.rs`
- `host/transport/src/bin/quic_server.rs`
- `host/transport/src/bin/transport_smoke_client.rs`
- `host/transport/README.md`
- `docs/transport-smoke-test.md`
- `docs/Status.md`
- `docs/execution-logs/phase-001.1-msquic-host-runtime.exec.md`

Files that should not change unless the implementation discovers a concrete incompatibility:

- `client-avp/Transport/*`
- `host/transport/src/protocol.rs`

## Step-by-Step Execution Plan

1. Remove the ambiguity between transcript scaffolding and runtime behavior.
   - Delete or demote `ListenerPlan`, `SmokeRoundTrip`, and similar transcript success surfaces from the default runtime path.
   - Preserve the current control-message schema and framing so the corrective subphase does not create avoidable client drift.

2. Fix the host crate's native dependency path first.
   - Update `host/transport/Cargo.toml` so the `msquic` crate resolves a real native MsQuic installation on Windows via the upstream-supported `find` flow.
   - Document the local prerequisite that `ms-quic` must be installed via vcpkg and discoverable through `VCPKG_ROOT`.
   - Keep the current version pin unless a missing API in that version blocks listener/stream work.

3. Rework configuration around a Windows-realistic certificate source.
   - Replace the current PFX/PEM-first server certificate assumption with a Windows certificate-hash configuration model.
   - Add explicit server certificate fields that are actually actionable for Schannel-backed MsQuic, for example:
     - certificate thumbprint/hash
     - store name, default `MY`
     - current-user vs machine-store toggle
   - Keep client-side debug validation as a simple enum or flag with only the minimum two modes needed now: system trust or debug insecure.
   - Remove or defer the current SHA-256 pinning path if it cannot be backed by real MsQuic certificate callbacks inside this milestone slice.

4. Turn `tls.rs` into a real MsQuic credential builder.
   - Convert `TransportServerConfig` into a concrete `msquic::CredentialConfig` plus `Credential` value instead of a summary string.
   - Use `Credential::CertificateHash` or `Credential::CertificateHashStore` for the server path.
   - Use `CredentialConfig::new_client()` for the smoke client and add `CredentialFlags::NO_CERTIFICATE_VALIDATION` only when the debug-insecure mode is selected.
   - Keep `tls.rs` responsible for all thumbprint parsing, store-selection logic, and credential-flag decisions so the rest of the transport code does not know about Schannel details.

5. Implement the real listener lifecycle in `server.rs`.
   - Build `Registration`, `Configuration`, and `Listener` using the existing ALPN and a small `Settings` object.
   - Open the listener on the configured bind address and port.
   - In the listener callback, accept exactly the connection shape Milestone 1 needs:
     - set the connection callback handler immediately
     - call `set_configuration` before returning
     - transfer accepted-connection ownership out of the callback safely so the connection remains alive after callback return
   - Expose a blocking server run method for the binary, such as `serve_until_roundtrip_complete()` or `serve_once()`, so the runtime can be locally validated without adding a higher-level async runtime.

6. Implement live connection and stream event handling in `connection.rs` plus `server.rs`.
   - Reuse `ControlConnection` to validate control-message order, but back it with live MsQuic events instead of transcript injection.
   - In `ConnectionEvent::PeerStreamStarted`, install the stream callback immediately and begin accumulating inbound bytes with `FrameAccumulator`.
   - In `StreamEvent::Receive`, decode frames, validate the `hello` message, send `hello_ack`, and optionally process `goodbye`.
   - In `StreamEvent::SendComplete`, reclaim the heap-owned send buffer.
   - In `StreamEvent::ShutdownComplete` and `ConnectionEvent::ShutdownComplete`, reclaim owned handles using the upstream-safe pattern instead of dropping them while the callback is still executing.

7. Implement a live host-local smoke client.
   - Build a real `TransportSmokeClient` over MsQuic `Connection::open`, `Connection::start`, and `Stream::open`.
   - On `Connected`, open one bidirectional stream, send `hello`, and keep the encoded send buffer alive until `SendComplete`.
   - On receipt of `hello_ack`, either:
     - send `goodbye` and gracefully close for the client-initiated-close case, or
     - wait for the server `goodbye` for the server-initiated-close case.
   - Signal success only after the client sees a real orderly shutdown event from MsQuic.

8. Make the binaries truthfully reflect live runtime state.
   - `quic_server` should log a real listening endpoint, accepted connection, stream start, message receipt, ack send, and shutdown result.
   - `transport_smoke_client` should log a real connection attempt, `hello` send, `hello_ack` receive, and the close direction exercised.
   - Return non-zero exit codes for handshake failure, ALPN mismatch, stream timeout, transport shutdown, or missing `hello_ack`.

9. Refresh the local validation artifacts.
   - Rewrite `host/transport/README.md` and `docs/transport-smoke-test.md` so they describe the live runtime prerequisites and commands, not transcript expectations.
   - Include exact PowerShell steps to:
     - verify the Windows version assumption
     - confirm `VCPKG_ROOT`
     - read the dev certificate thumbprint from the Windows certificate store
     - run unit tests
     - run the server
     - run the client in both close modes
   - State clearly that the AVP Swift files remain first-pass scaffolding and are not part of this corrective subphase.

10. Correct status and logging after local execution.
   - Add `docs/execution-logs/phase-001.1-msquic-host-runtime.exec.md` with the actual implementation result and actual local validation result.
   - Update `docs/Status.md` to say one of two things only:
     - Milestone 1 host runtime is now live-validated on Windows, or
     - the host runtime code exists but Milestone 1 still remains incomplete pending local live validation.
   - Do not leave wording that implies transcript success is close enough to milestone completion.

## Validation Steps

1. Verify local prerequisites on a real Windows machine.
   - Windows 11 or Windows Server 2022 or newer.
   - Rust toolchain and Cargo.
   - vcpkg installed, with `ms-quic` installed for `x64-windows` and `VCPKG_ROOT` set.
   - A development certificate imported into `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My`.

2. Verify the certificate thumbprint that the server will use.
   - Example PowerShell shape:
     - `Get-ChildItem Cert:\CurrentUser\My | Select-Object Subject, Thumbprint`
   - Confirm the thumbprint configured in `HOLOBRIDGE_TRANSPORT_CERT_SHA1` matches an installed certificate.

3. Run crate-level tests before live networking.
   - `Set-Location host/transport`
   - `cargo test`
   - Expected result: framing/state tests still pass and any new config-mapping tests pass.

4. Run the live server.
   - Example local environment:
     - `Set-Location host/transport`
     - `$env:VCPKG_ROOT = "C:\path\to\vcpkg"`
     - `$env:HOLOBRIDGE_TRANSPORT_BIND = "127.0.0.1"`
     - `$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"`
     - `$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = "<windows-cert-thumbprint>"`
     - `$env:HOLOBRIDGE_TRANSPORT_CERT_STORE = "MY"`
     - `$env:HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE = "false"`
     - `cargo run --bin quic_server`
   - Expected result: the process reports a live listener and remains ready for a connection.

5. Run the live smoke client for client-initiated close.
   - Example local environment:
     - `Set-Location host/transport`
     - `$env:VCPKG_ROOT = "C:\path\to\vcpkg"`
     - `$env:HOLOBRIDGE_TRANSPORT_HOST = "127.0.0.1"`
     - `$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"`
     - `$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = "true"`
     - `$env:HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE = "true"`
     - `cargo run --bin transport_smoke_client`
   - Expected live log sequence:
     - listener started
     - connection accepted
     - connected
     - peer stream started
     - `hello` received on server
     - `hello_ack` sent by server
     - `hello_ack` received by client
     - `goodbye` sent by client
     - orderly stream and connection shutdown on both sides

6. Run the live smoke client for server-initiated close.
   - Server:
     - set `$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = "true"`
   - Client:
     - omit or disable client goodbye
   - Expected live log sequence:
     - client receives `hello_ack`
     - server sends `goodbye`
     - client reports orderly remote close
     - no timeout, leaked handle, or transport-shutdown error

7. Treat the following as failure conditions.
   - Listener never starts or binds a real port.
   - `NEW_CONNECTION` arrives but handshake stalls because configuration was not set inside acceptance flow.
   - `PEER_STREAM_STARTED` arrives but the app does not install the stream callback immediately.
   - Any send buffer is freed before `SendComplete`.
   - Any connection or stream handle is dropped from inside its active callback and causes undefined cleanup behavior.
   - `hello_ack` is never received.
   - Either close direction ends in transport error, timeout, or leaked state.

8. Update documentation only after recording the actual result.
   - Write the real command outcomes and any deviations into `docs/execution-logs/phase-001.1-msquic-host-runtime.exec.md`.
   - Update `docs/Status.md` with those real outcomes.

## Risks and Caveats

- The MsQuic Rust binding is still officially experimental, so callback lifetime and ownership discipline is the highest-risk implementation area.
- The current repo config assumes PFX/PEM inputs, but a Windows Schannel-backed MsQuic runtime is better served by certificate-store hash selection. This is the most important config correction in the subphase.
- Using vcpkg plus the crate's `find` feature is the narrowest local path, but it adds an external machine prerequisite that must be documented exactly.
- A debug client mode that disables certificate validation is acceptable only for the local smoke client in Milestone 1 validation. It must be labeled as a development-only path and not expanded into later auth/session work.
- Custom certificate pinning via MsQuic certificate callbacks is possible, but it is not part of the minimum Milestone 1 acceptance slice and would add unnecessary callback complexity.
- The AVP transport files remain scaffold-only after this subphase. Milestone 1 as a whole should not be marked complete until the team decides the host-local smoke path is sufficient or separately validates the Apple runtime path.

## Defaults Assumed

- The corrective subphase keeps ALPN `holobridge-m1`, protocol version `1`, and the existing `hello` / `hello_ack` / `goodbye` schema.
- The first live validation target is localhost on `127.0.0.1:4433`.
- A single bidirectional control stream per connection is enough for this milestone slice.
- The default server certificate source is a certificate already imported into `CurrentUser\My` on Windows.
- The first local smoke path may use `NO_CERTIFICATE_VALIDATION` on the client side if that is the fastest way to verify the live transport runtime.
- No AVP Swift transport changes are required unless the host runtime implementation reveals an actual control-contract mismatch.

## Blocking Questions

None