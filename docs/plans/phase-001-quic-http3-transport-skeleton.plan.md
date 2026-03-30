# Phase Plan: Milestone 1 – QUIC / HTTP3 Transport Skeleton

## Goal

Establish a minimal end-to-end QUIC transport skeleton that lets the Windows host accept a connection, lets the AVP client or a host-local smoke client connect, exchanges one reliable control message round-trip, and closes cleanly from either side. Favor a host-first slice that can be validated on Windows without depending on AVP runtime access in this session.

## Acceptance Criteria

- Host can accept a QUIC connection from the AVP client or a test client.
- Client can establish a QUIC connection to the host.
- A simple control message can be sent from client to host and a response sent host to client.
- Connection can be closed cleanly from either side.

## Relevant Existing Context

- `docs/Plan.md` defines Milestone 1 as a transport-only slice and explicitly excludes auth and video.
- `docs/streaming-v1.md` and `docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md` require HTTP/3 + QUIC as the transport direction, with reliable QUIC streams for control and datagrams reserved for later video milestones.
- `docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md` already recommends MsQuic on Windows and Network.framework on Apple platforms.
- `host.instructions.md` requires a Rust-first host, safe Rust where practical, and strict concern separation with transport isolated under `host/transport/`.
- `client-avp.instructions.md` requires a native visionOS/Swift client, transport isolated under `client-avp/Transport/`, and no WebRTC/RTP fallback.
- The repo is still scaffolding-only: `host/` and `client-avp/` contain only README placeholders, and `docs/plans/` plus `docs/execution-logs/` are empty.
- This browser/tunnel session currently cannot launch terminal commands because of a workspace URI/provider error, so the milestone plan must not assume the orchestrator can run local shell commands from here.
- AVP runtime validation cannot happen from this Windows session. The user explicitly prefers host-first momentum plus a strong client contract if full AVP runtime verification is deferred.

## Verified Findings

- ADR 0001 already settles the transport family decision: QUIC is mandatory, and RTP/RTSP/WebRTC must not be introduced.
- Existing repo docs consistently describe control traffic as reliable QUIC stream traffic and defer video to later QUIC datagram work, so Milestone 1 should stay narrowly focused on stream-based control.
- The official MsQuic repo exposes the exact listener, connection, stream open/start/send/shutdown lifecycle needed for this milestone, and its official sample app implements the same basic flow: connect, open one bidirectional stream, send data, respond, and shut down.
- The official MsQuic README advertises a Rust interop layer and docs.rs package, but the official `docs/BUILD.md` states that Rust support is experimental and not officially supported. That makes MsQuic the right host engine, but it also means the Rust-facing boundary must stay narrow so a direct C FFI fallback is cheap if needed.
- The official MsQuic build/docs flow supports preinstalled binaries and vcpkg on Windows. For Milestone 1, using a preinstalled/dev MsQuic binary is lower risk than source-building MsQuic inside this repo.
- Search of the official `apple/swift-nio-transport-services` repo did not surface a clear QUIC-specific stream abstraction. Its public bootstraps and channel types are explicitly TCP/UDP-oriented around Network.framework. That makes NIOTS a poor choice for the first custom QUIC skeleton here.
- Network.framework remains the correct Apple-side dependency surface because it is first-party, native to Apple platforms, and aligned with the existing ADR. Runtime behavior still needs later verification on Mac/Xcode/visionOS.
- The current repo docs say “HTTP/3 + QUIC” at the architecture level, but the milestone acceptance criteria only require a QUIC connection plus a reliable control stream round-trip. The smallest execution-ready slice is therefore a direct QUIC control-stream skeleton with an application ALPN, not a full HTTP/3 request/response layer.

## Recommended Technical Approach

Build this milestone as a direct QUIC control-stream skeleton that preserves the repo’s HTTP/3 + QUIC direction without forcing full HTTP/3 request routing before it is needed. The host should be the primary executable validation path in Milestone 1; the AVP code should mirror the same control contract and be structured for later Xcode validation.

- Host transport engine: use the official `msquic` Rust package from crates.io, pinned to the latest stable published version at implementation time rather than left floating. Treat it as the preferred host binding, but isolate all MsQuic-specific code behind a tiny backend boundary because the official MsQuic docs classify Rust support as experimental.
- Host fallback path: if the Rust binding blocks implementation in practice, fall back to a narrow Rust FFI layer over the stable C MsQuic API inside `host/transport/` instead of switching to a different QUIC library. That preserves the Windows/MsQuic architecture choice and avoids later transport churn.
- Client transport engine: use direct Apple `Network.framework` APIs rather than `swift-nio-transport-services`. Keep the Apple-specific implementation behind a `TransportClient` abstraction so the first-pass code can be authored now and runtime-verified later on Mac/Xcode.
- Control channel shape: use one bidirectional reliable QUIC stream per connection for Milestone 1.
- ALPN: use a temporary application ALPN such as `holobridge-m1` for the transport skeleton. Do not pretend Milestone 1 is already speaking a full HTTP/3 application protocol if the implementation is only using raw QUIC stream primitives.
- Message framing: use a 4-byte big-endian length prefix followed by a UTF-8 JSON payload. This keeps the first control contract readable and cross-language, while avoiding delimiter edge cases.
- Message set: keep the control protocol to three messages only.
  - `hello`: sent by the client after the control stream opens.
  - `hello_ack`: sent by the host in response.
  - `goodbye`: optional close-intent message sent by whichever side initiates clean shutdown.
- Shutdown behavior: either side may initiate close by sending `goodbye`, half-closing its send side, and then shutting down the QUIC connection with application error code `0` after the peer has drained or after a short timeout.
- Dev TLS: the host must load a real dev certificate from config. Do not invent secrets or production certs. For Milestone 1 validation, prefer trusting the specific dev cert or pinned fingerprint in smoke paths. If that proves too expensive with the experimental Rust binding, allow a clearly labeled debug-only validation bypass that is isolated behind config and never treated as production behavior.
- Scope guardrails: do not add Sign in with Apple, resume tokens, video datagrams, UI work, or capture/decode hooks in this milestone. Only add the interfaces and TODO markers necessary to keep later milestones from having to rewrite the transport boundary.

Recommended control message schema for both host and client:

```json
{"type":"hello","protocol_version":1,"client_name":"transport-smoke","capabilities":["control-stream-v1"]}
```

```json
{"type":"hello_ack","protocol_version":1,"message":"ok"}
```

```json
{"type":"goodbye","reason":"client-close"}
```

## Likely Files and Modules to Change

- `host/transport/Cargo.toml`
- `host/transport/src/lib.rs`
- `host/transport/src/config.rs`
- `host/transport/src/protocol.rs`
- `host/transport/src/tls.rs`
- `host/transport/src/server.rs`
- `host/transport/src/connection.rs`
- `host/transport/src/bin/quic_server.rs`
- `host/transport/src/bin/transport_smoke_client.rs`
- `host/transport/tests/codec_roundtrip.rs`
- `host/transport/tests/loopback_smoke.rs` if a deterministic live test is practical; otherwise document the smoke path instead of forcing a brittle checked-in integration test.
- `host/transport/README.md`
- `client-avp/Transport/TransportConfiguration.swift`
- `client-avp/Transport/ControlMessage.swift`
- `client-avp/Transport/TransportClient.swift`
- `client-avp/Transport/NetworkFrameworkQuicClient.swift`
- `client-avp/Transport/README.md`
- `docs/transport-smoke-test.md`
- `docs/Status.md`
- `docs/adr/0003-transport-skeleton-library-choices.md` only if implementation has to deviate from MsQuic + Network.framework or if the team decides the raw-QUIC-skeleton-vs-full-H3 distinction should be formalized.

## Step-by-Step Execution Plan

1. Lock the milestone scope before writing code.
   - Treat Milestone 1 as a transport skeleton only.
   - Explicitly defer auth, video, datagrams, resume logic, and UI.
   - Record in the implementation notes that Milestone 1 uses a direct QUIC control-stream skeleton beneath the broader HTTP/3 + QUIC architecture.

2. Scaffold `host/transport/` as a standalone Rust crate.
   - Create `host/transport/` as the first real Rust deliverable so the folder path matches the milestone deliverable text.
   - Add a library target plus small binaries for server and smoke client.
   - Add only the dependencies needed for Milestone 1: `msquic`, `serde`, `serde_json`, `tracing`, and `tracing-subscriber` are enough for the expected scope.
   - Keep async/runtime choices conservative. Do not pull in Tokio unless the chosen MsQuic binding or shutdown coordination genuinely needs it.

3. Define the host transport configuration and control contract.
   - Add `TransportServerConfig` with bind address, port, ALPN, certificate source, and debug validation settings.
   - Add a `ControlMessage` enum plus encode/decode helpers in `protocol.rs`.
   - Add unit tests that verify encode/decode round-trips for `hello`, `hello_ack`, and `goodbye`, plus at least one malformed-frame case.

4. Implement the MsQuic-backed host server lifecycle.
   - Initialize MsQuic registration and configuration.
   - Load the development certificate from config.
   - Start a listener on a local development endpoint.
   - On `NEW_CONNECTION`, attach connection callbacks and accept the connection.
   - Limit the skeleton to one bidirectional peer control stream.
   - On `PEER_STREAM_STARTED`, attach the stream callback, read the inbound `hello`, send `hello_ack`, and track orderly shutdown.
   - Confine raw MsQuic handles, callbacks, and unsafe edges to `tls.rs`, `connection.rs`, and the smallest possible helper code.

5. Add a host-local smoke client as the primary validation path.
   - Create `transport_smoke_client.rs` in the same crate so the host team can validate Milestone 1 on Windows without waiting for AVP runtime.
   - The smoke client should connect, open the control stream, send `hello`, wait for `hello_ack`, then optionally send `goodbye` and close.
   - If deterministic live integration testing with cert setup is straightforward, add `tests/loopback_smoke.rs`; if not, do not waste time fighting infra. Use the smoke client binary plus a manual doc as the required validation path.

6. Document the Windows validation path.
   - Add `host/transport/README.md` and `docs/transport-smoke-test.md`.
   - Document certificate prerequisites, configuration fields, exact commands to run from a local terminal, and expected log lines for success and failure.
   - Include both “client initiates close” and “server initiates close” scenarios so clean shutdown is verified from either side.

7. Author the AVP-side transport contract before the full transport implementation.
   - Mirror the JSON message schema and protocol version in Swift.
   - Define a small `TransportClient` abstraction with methods like `connect()`, `sendHello()`, `awaitHelloAck()`, and `close()`.
   - Keep this API independent from Sign in with Apple, SwiftUI views, and future decode/display code.

8. Implement the Apple transport adapter directly on Network.framework.
   - Use direct `Network.framework` QUIC support rather than NIOTS.
   - Configure the same ALPN and use the same message framing as the host.
   - Implement only the minimum client flow needed for Milestone 1: connect, open the control path, send `hello`, read `hello_ack`, and close cleanly.
   - Leave precise Apple runtime checks in well-named TODOs only where Mac/Xcode verification is genuinely required. Do not leave vague placeholders.

9. Finish milestone-level docs and status.
   - Add the manual smoke-test document even if automated tests also exist, because this session cannot run local commands.
   - Update `docs/Status.md` after implementation with what changed, what was validated, what could not be validated here, and the next milestone recommendation.
   - Add a new ADR only if the real implementation has to deviate from the choices above or if the raw-QUIC-skeleton decision needs to be explicitly recorded.

## Validation Steps

1. Verify artifact completeness.
   - Confirm the files above exist.
   - Confirm both host and client use the same ALPN, message schema, and protocol version.

2. Verify host codec behavior.
   - When local terminal access is available, run the host crate tests and confirm codec/unit tests pass before live networking is attempted.

3. Verify Windows localhost round-trip using the host smoke path.
   - Start the host server on a local endpoint with the configured dev certificate.
   - Run the host smoke client against it.
   - Confirm the expected sequence in logs: listener started, connection accepted, `hello` received, `hello_ack` sent, ack received by client, orderly stream shutdown, orderly connection shutdown.

4. Verify client-initiated close.
   - Run the smoke client so it sends `goodbye` and closes after the ack.
   - Confirm the host reports a clean peer shutdown instead of timeout, transport error, or leaked connection state.

5. Verify server-initiated close.
   - Trigger a server-side close after `hello_ack` is sent, either through a CLI flag or a small test-mode timer.
   - Confirm the client reports an orderly close and the host releases the connection cleanly.

6. Verify later Apple-side runtime behavior on Mac/Xcode.
   - Build the `client-avp/Transport/` code into a small test harness or app target.
   - Connect to the Windows host using the same dev certificate expectations.
   - Confirm `hello`/`hello_ack` round-trip and both close directions.
   - Record the actual results in `docs/Status.md`.

7. Respect the current environment constraint.
   - Because this browser/tunnel session cannot launch commands and cannot run visionOS code, do not claim live validation from this session.
   - Milestone 1 execution from this environment should end with checked-in validation artifacts and exact manual steps, not with unverified command claims.

## Risks and Caveats

- Official MsQuic Rust support is experimental, so the most important implementation discipline is to keep the binding-specific code isolated.
- Development certificate provisioning and trust is the most likely source of early failures on both Windows and Apple validation paths.
- The AVP transport code can be authored now, but it cannot be considered runtime-validated until a Mac/Xcode pass is done.
- If the team later insists that Milestone 1 must already use explicit HTTP/3 application framing rather than direct QUIC control streams, the plan should be revisited before implementation starts. That is not a blocker with the current repo docs, but it is a real architectural pivot if requested.
- Do not let Milestone 1 sprawl into auth, video transport, or UI glue. That would slow validation and create unnecessary rework.

## Defaults Assumed

- Local development uses `127.0.0.1:4433` as the default bind endpoint.
- The skeleton uses one bidirectional reliable control stream per QUIC connection.
- Control messages use 4-byte length-prefixed UTF-8 JSON in Milestone 1.
- `client-avp/Transport/` can be authored as plain Swift source files before a full Xcode/visionOS app project exists.
- A documented manual smoke test is acceptable if a deterministic live MsQuic integration test is not practical in the current environment.

## Blocking Questions

None.