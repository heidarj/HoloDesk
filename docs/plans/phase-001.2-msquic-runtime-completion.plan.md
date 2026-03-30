# Phase Plan: Phase 001.2 - MsQuic Runtime Completion

## Goal

Complete the missing host-side live MsQuic runtime for Milestone 1 on this Windows machine by replacing the current runtime-unavailable stubs with a real MsQuic listener and a real MsQuic smoke client, preserving the existing control protocol and certificate-store configuration work from Phase 001.1. This corrective subphase is only about host transport runtime completion and truthful validation for the Milestone 1 smoke path.

## Acceptance Criteria

- Real MsQuic listener on Windows.
- Real MsQuic smoke client.
- Control stream `hello` -> `hello_ack` round-trip.
- Clean close from both client-initiated and server-initiated paths.
- Truthful docs, status, and execution logging only after actual validation.

## Relevant Existing Context

- [docs/plans/phase-001.1-msquic-host-runtime.plan.md](docs/plans/phase-001.1-msquic-host-runtime.plan.md) already defined the intended live runtime shape, but its dependency guidance is now stale because the pinned crate version does not resolve.
- [docs/execution-logs/phase-001.1-msquic-host-runtime.exec.md](docs/execution-logs/phase-001.1-msquic-host-runtime.exec.md) records that Phase 001.1 stopped at truthful config and documentation cleanup and never produced a live listener, client, or stream round-trip.
- [docs/Status.md](docs/Status.md) correctly marks Milestone 1 as incomplete and says the checked-in host transport still lacks a live MsQuic runtime.
- [docs/Plan.md](docs/Plan.md) keeps Milestone 1 narrowly scoped to connection setup, one control round-trip, and clean close.
- [docs/streaming-v1.md](docs/streaming-v1.md) and [docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md](docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md) require QUIC control transport on Windows and do not require auth, session, video, or AVP runtime work in this milestone slice.
- [host/transport/src/protocol.rs](host/transport/src/protocol.rs) already contains the JSON control contract, ALPN, protocol version, and frame codec needed for the smoke path.
- [host/transport/src/connection.rs](host/transport/src/connection.rs) already contains the control-stream state machine for `hello`, `hello_ack`, and `goodbye`.
- [host/transport/src/config.rs](host/transport/src/config.rs) and [host/transport/src/tls.rs](host/transport/src/tls.rs) already moved toward a Windows certificate-store thumbprint model and should be extended, not replaced.
- [host/transport/src/server.rs](host/transport/src/server.rs), [host/transport/src/bin/quic_server.rs](host/transport/src/bin/quic_server.rs), and [host/transport/src/bin/transport_smoke_client.rs](host/transport/src/bin/transport_smoke_client.rs) currently stop at configuration summaries and return runtime-unavailable errors instead of creating MsQuic objects.

## Verified Findings

- This machine is Windows, terminal execution now works, Visual Studio Build Tools with the VC++ workload is installed, and `rustup`, `rustc`, and `cargo` are installed and working.
- Local validation should treat `VCPKG_ROOT` as `C:\Users\heida\vcpkg`.
- Native MsQuic is already installed via vcpkg, and the verified local runtime artifacts are:
  - `C:\Users\heida\vcpkg\installed\x64-windows\lib\msquic.lib`
  - `C:\Users\heida\vcpkg\installed\x64-windows\bin\msquic.dll`
- `cargo build` in `host/transport` currently fails before compilation because [host/transport/Cargo.toml](host/transport/Cargo.toml) pins `msquic = "=2.4.0"`, which no longer resolves on crates.io.
- Cargo reported published versions such as `2.5.1-beta`, `2.5.0-beta4`, and `2.5.0-beta3`. The corrective subphase must not keep the `=2.4.0` pin.
- Official crates.io and docs.rs pages show the currently published crate line is `msquic 2.5.1-beta`, and its feature list includes `find`, `src`, `overwrite`, `preview-api`, `quictls`, and `static`.
- Official MsQuic build docs state Rust support is experimental. That is acceptable for this milestone only if the unsafe and callback-sensitive code stays isolated to the host transport runtime boundary.
- Official MsQuic docs state listener callbacks may begin before `ListenerStart` returns, and the server must set the accepted connection callback and call `ConnectionSetConfiguration` during acceptance or the handshake can stall and time out.
- The current repo already has the right non-runtime building blocks for the smoke path: framing in [host/transport/src/protocol.rs](host/transport/src/protocol.rs), message-order validation in [host/transport/src/connection.rs](host/transport/src/connection.rs), and Windows certificate thumbprint parsing in [host/transport/src/tls.rs](host/transport/src/tls.rs).
- The implementer must stay within workspace files plus official docs and terminal validation. Do not browse cargo caches, `.cargo`, package registries, vcpkg trees, user-profile caches, or any other off-workspace directory to rediscover APIs or native artifacts.

## Recommended Technical Approach

Treat this as a narrow runtime completion pass, not a redesign.

- Fix the dependency root cause first.
  - Change [host/transport/Cargo.toml](host/transport/Cargo.toml) from `msquic = { version = "=2.4.0", default-features = false, features = ["find"] }` to `msquic = { version = "=2.5.1-beta", default-features = false, features = ["find"] }`.
  - Keep `default-features = false` so this subphase continues using the preinstalled native MsQuic from vcpkg instead of broadening into source builds.
  - Do not use vague version ranges or leave the dependency on `latest`; keep the exact pin at `=2.5.1-beta` so the binding surface is stable during this corrective pass.
- Keep the existing Windows certificate-store model.
  - Continue using SHA-1 thumbprint plus store selection from [host/transport/src/config.rs](host/transport/src/config.rs).
  - Extend [host/transport/src/tls.rs](host/transport/src/tls.rs) from summary-only binding objects into real `CredentialConfig` and `Credential` builders for both server and client.
  - Keep the local smoke client's insecure validation bypass behind `HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT=true` only for localhost validation.
- Implement the live runtime in the narrowest modules possible.
  - Use [host/transport/src/server.rs](host/transport/src/server.rs) for `Registration`, `Configuration`, `Listener`, `Connection`, and `Stream` ownership plus callback wiring.
  - Reuse [host/transport/src/connection.rs](host/transport/src/connection.rs) for application-level control-state transitions instead of duplicating protocol logic in callbacks.
  - Keep [host/transport/src/protocol.rs](host/transport/src/protocol.rs) unchanged unless a real runtime bug forces a framing fix.
- Build only the Milestone 1 live path.
  - One MsQuic registration.
  - One configuration for ALPN `holobridge-m1`.
  - One listener on localhost.
  - One bidirectional control stream.
  - `hello` receive, `hello_ack` send, and `goodbye` handling for both close directions.
  - No datagrams, HTTP/3 layering, auth, resume tokens, video, or AVP changes.
- Isolate callback-risk areas deliberately.
  - Install the accepted connection callback and apply configuration immediately during the listener acceptance event.
  - Install the stream callback immediately on peer stream start.
  - Keep outbound frame buffers heap-owned until MsQuic reports send completion.
  - Reclaim connection and stream ownership only on shutdown-complete style events or after callback return, not by dropping active handles inside their running callbacks.
- Keep docs and status truthful.
  - Update [host/transport/README.md](host/transport/README.md), [docs/transport-smoke-test.md](docs/transport-smoke-test.md), and [docs/Status.md](docs/Status.md) only after actual build and smoke validation completes.
  - If validation fails, record the real failure in a new execution log and leave Milestone 1 incomplete.

## Likely Files and Modules to Change

- [host/transport/Cargo.toml](host/transport/Cargo.toml)
- [host/transport/src/server.rs](host/transport/src/server.rs)
- [host/transport/src/connection.rs](host/transport/src/connection.rs)
- [host/transport/src/tls.rs](host/transport/src/tls.rs)
- [host/transport/src/config.rs](host/transport/src/config.rs)
- [host/transport/src/bin/quic_server.rs](host/transport/src/bin/quic_server.rs)
- [host/transport/src/bin/transport_smoke_client.rs](host/transport/src/bin/transport_smoke_client.rs)
- [host/transport/src/lib.rs](host/transport/src/lib.rs) only if new runtime types or errors must be re-exported
- [host/transport/README.md](host/transport/README.md)
- [docs/transport-smoke-test.md](docs/transport-smoke-test.md)
- [docs/Status.md](docs/Status.md)
- `docs/execution-logs/phase-001.2-msquic-runtime-completion.exec.md` as the new corrective execution log

Files that should stay out of scope unless a real contract mismatch is discovered:

- [client-avp/Transport/ControlMessage.swift](client-avp/Transport/ControlMessage.swift)
- [client-avp/Transport/NetworkFrameworkQuicClient.swift](client-avp/Transport/NetworkFrameworkQuicClient.swift)
- [client-avp/Transport/TransportClient.swift](client-avp/Transport/TransportClient.swift)
- [client-avp/Transport/TransportConfiguration.swift](client-avp/Transport/TransportConfiguration.swift)
- [docs/Plan.md](docs/Plan.md)
- [docs/streaming-v1.md](docs/streaming-v1.md)

## Step-by-Step Execution Plan

1. Fix the unresolved crate pin before touching runtime code.
   - Edit [host/transport/Cargo.toml](host/transport/Cargo.toml) to pin `msquic` to `=2.5.1-beta` with `default-features = false` and `features = ["find"]`.
   - Do not change the rest of the dependency set unless compile errors prove a secondary crate update is required.

2. Prove the build now reaches Rust compilation.
   - From `C:\Users\heida\source\HoloDesk\host\transport`, run `cargo build` with `VCPKG_ROOT=C:\Users\heida\vcpkg`.
   - If the build still fails before Rust compilation, stop and fix only the dependency or native discovery issue first. Do not start listener code changes while the crate still fails at dependency resolution.

3. Replace summary-only TLS bindings with real MsQuic credential builders.
   - In [host/transport/src/tls.rs](host/transport/src/tls.rs), convert the existing thumbprint/store parsing into functions that return real MsQuic credential objects for:
     - server credential config using the Windows certificate store
     - client credential config using either system trust or debug-insecure mode
   - Keep all Schannel-specific mapping logic isolated in this file.

4. Implement the server runtime object graph.
   - In [host/transport/src/server.rs](host/transport/src/server.rs), replace `serve_once` with a live Windows-only path that creates a MsQuic registration, configuration, and listener.
   - Start the listener on the configured bind address and port using the existing ALPN.
   - Keep runtime state scoped to one smoke-test connection and one control stream.

5. Implement accepted-connection handling safely.
   - In the listener callback, immediately install the connection callback on the accepted connection.
   - Apply the configuration during the acceptance flow so the handshake can proceed.
   - Store connection ownership in runtime state that outlives the callback.

6. Implement live control-stream processing.
   - In [host/transport/src/connection.rs](host/transport/src/connection.rs), keep `ControlConnection` as the application state machine and adapt it for live receive/send integration.
   - On server stream receive, feed bytes into `FrameAccumulator`, decode `hello`, generate `hello_ack`, and queue a `goodbye` only when the configured close mode requires it.
   - On client stream receive, validate `hello_ack`, then either send `goodbye` or wait for the server close path based on configuration.

7. Implement real send and shutdown handling.
   - Keep encoded frame buffers alive until send completion.
   - Track stream shutdown and connection shutdown explicitly so each path reports a real clean close.
   - Map transport failures, protocol failures, and timeout-like outcomes to non-zero process exits.

8. Turn the binaries into real runtime entry points.
   - Update [host/transport/src/bin/quic_server.rs](host/transport/src/bin/quic_server.rs) so it logs listener start, accepted connection, control-stream events, and orderly shutdown from live MsQuic callbacks.
   - Update [host/transport/src/bin/transport_smoke_client.rs](host/transport/src/bin/transport_smoke_client.rs) so it logs connection start, stream open, `hello` send, `hello_ack` receive, close direction, and exit status from the live runtime.

9. Refresh the validation docs only after the live path works.
   - Update [host/transport/README.md](host/transport/README.md) and [docs/transport-smoke-test.md](docs/transport-smoke-test.md) with the exact commands that were actually used.
   - Add the corrective execution log file with the real command results.
   - Update [docs/Status.md](docs/Status.md) only after both close directions have been exercised or explicitly record which one still fails.

## Validation Steps

Use PowerShell and do not substitute a different machine path during this subphase.

1. Build after fixing the crate version.

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
cargo build
```

Expected result: dependency resolution succeeds and the build reaches Rust compilation with the published `msquic` crate line.

2. Run the existing crate tests.

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
cargo test
```

Expected result: protocol framing and control-message tests pass.

3. Inspect the available server certificate thumbprint in the Windows certificate store.

```powershell
Get-ChildItem Cert:\CurrentUser\My | Select-Object Subject, Thumbprint
```

Use the selected thumbprint in the server command below. Use `Cert:\LocalMachine\My` only if the chosen certificate actually lives there.

4. Run the live server for the client-initiated close case.

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_BIND = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = "<thumbprint>"
$env:HOLOBRIDGE_TRANSPORT_CERT_STORE = "MY"
$env:HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE = "false"
$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = "false"
cargo run --bin quic_server
```

Expected result: the server reports a real listener start and waits for a client.

5. Run the live smoke client for client-initiated close in a second terminal.

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_HOST = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = "true"
$env:HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE = "true"
cargo run --bin transport_smoke_client
```

Expected result:

- real QUIC connection established
- real peer stream start
- server receives `hello`
- client receives `hello_ack`
- client sends `goodbye`
- both sides log orderly stream and connection shutdown

6. Repeat for server-initiated close.

Server terminal:

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_BIND = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_CERT_SHA1 = "<thumbprint>"
$env:HOLOBRIDGE_TRANSPORT_CERT_STORE = "MY"
$env:HOLOBRIDGE_TRANSPORT_CERT_MACHINE_STORE = "false"
$env:HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK = "true"
cargo run --bin quic_server
```

Client terminal:

```powershell
Set-Location C:\Users\heida\source\HoloDesk\host\transport
$env:VCPKG_ROOT = "C:\Users\heida\vcpkg"
$env:HOLOBRIDGE_TRANSPORT_HOST = "127.0.0.1"
$env:HOLOBRIDGE_TRANSPORT_PORT = "4433"
$env:HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT = "true"
$env:HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE = "false"
cargo run --bin transport_smoke_client
```

Expected result:

- client receives `hello_ack`
- server sends `goodbye`
- client reports orderly remote close
- both processes exit successfully

7. Treat any of the following as validation failure.

- dependency resolution still fails because the crate pin was not corrected
- native MsQuic discovery fails with `VCPKG_ROOT=C:\Users\heida\vcpkg`
- listener starts but no accepted connection reaches the callback
- handshake stalls because configuration is not applied during acceptance
- stream callback is not installed on peer stream start
- `hello_ack` is never received
- either close direction ends in timeout, abrupt transport shutdown, or non-orderly termination

8. Update the docs only after actual runs.

- Record the exact command outcomes in `docs/execution-logs/phase-001.2-msquic-runtime-completion.exec.md`.
- Update [docs/Status.md](docs/Status.md) with the real outcome, not the intended one.

## Risks and Caveats

- `msquic 2.5.1-beta` is a published crate version, but the Rust binding remains experimental, so callback ownership and send-buffer lifetime handling are still the highest-risk code paths.
- The `find` feature avoids broadening into source builds, but it depends on the existing native MsQuic install and `VCPKG_ROOT` being set exactly to `C:\Users\heida\vcpkg` during validation.
- Windows Schannel-backed MsQuic requires a Windows version with TLS 1.3 support. The user has already verified the machine is Windows and suitable for local terminal validation, so this subphase should not add alternate TLS provider work.
- The smoke client may use insecure certificate validation for localhost only. Do not expand that bypass beyond this local validation slice.
- Do not update milestone status optimistically. If one close direction still fails, keep Milestone 1 incomplete and log the exact failure.

## Defaults Assumed

- The existing control contract stays unchanged: ALPN `holobridge-m1`, protocol version `1`, framing with a 4-byte big-endian length prefix, and messages `hello`, `hello_ack`, and `goodbye`.
- Local validation stays on `127.0.0.1:4433`.
- The server certificate comes from `Cert:\CurrentUser\My` unless the implementer intentionally sets the machine-store flag.
- The first validation pass uses `HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT=true` on the smoke client to avoid broadening scope into trust-store setup work.

## Blocking Questions

None