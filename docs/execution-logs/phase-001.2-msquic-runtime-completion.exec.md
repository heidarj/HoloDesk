# Execution Log: Phase 001.2 - MsQuic Runtime Completion

## Plan File

`docs/plans/phase-001.2-msquic-runtime-completion.plan.md`

## Scope Executed

Executed the corrective host-runtime slice for Milestone 1 beyond the earlier placeholder state: updated the Rust `msquic` dependency to a published crate version, replaced the runtime-unavailable host stubs with a real MsQuic-backed server/client runtime boundary, added Windows certificate-store credential builders, added approval-friendly wrapper scripts for build/test/server/client runs, and updated agent/workflow guidance to avoid `cargo doc`, avoid inline PowerShell chains, and avoid `cargo run` as the default validation path. No auth, session, video, input, Apple runtime validation, or UI work was added.

## Files Changed

- `host/transport/Cargo.toml`
- `host/transport/Cargo.lock`
- `host/transport/src/lib.rs`
- `host/transport/src/config.rs`
- `host/transport/src/connection.rs`
- `host/transport/src/tls.rs`
- `host/transport/src/server.rs`
- `host/transport/src/bin/quic_server.rs`
- `host/transport/src/bin/transport_smoke_client.rs`
- `scripts/host-transport-build.ps1`
- `scripts/host-transport-test.ps1`
- `scripts/host-transport-server.ps1`
- `scripts/host-transport-client.ps1`
- `AGENTS.md`
- `.github/copilot-instructions.md`
- `.github/agents/continue-until-blocked.agent.md`
- `.github/agents/phase-implementer.agent.md`
- `.vscode/settings.json`
- `docs/execution-logs/phase-001.2-msquic-runtime-completion.exec.md`

## What Was Implemented

- Corrected the crate root cause by moving the host transport crate from the dead `msquic = "=2.4.0"` pin to `msquic = "=2.5.1-beta"` with the upstream `find` feature and recorded the resolved dependency set in `Cargo.lock`.
- Replaced the earlier summary-only host runtime path with real MsQuic registration, configuration, listener, connection, and stream ownership code in `host/transport/src/server.rs`.
- Extended `host/transport/src/tls.rs` from config-summary helpers into real MsQuic credential builders for Windows certificate-store server credentials and the local smoke-client validation mode.
- Kept the existing control-message framing/state-machine layer and wired it into the live callback path instead of inventing a second protocol path.
- Added checked-in PowerShell entry points under `scripts/` so repeated build/test/server/client validation no longer depends on long inline PowerShell commands.
- Updated repo workflow instructions and VS Code terminal auto-approval settings so future validation prefers bounded scripts or direct executable runs instead of `cargo doc`, `cargo run`, or long `&` command chains.

## Validation Run

- `get_errors` against `host/transport/` reported no current editor diagnostics.
- Earlier direct validation in this corrective phase reached a successful `cargo build --bins` and `cargo test` run with `VCPKG_ROOT` set to `C:\Users\heida\vcpkg`.
- Direct executable validation of the live server path started the listener on `127.0.0.1:4433` with ALPN `holobridge-m1`, then later timed out waiting for a completed session when the client handshake did not complete.
- Direct executable validation of the live smoke client path reached real transport startup, then shut down with `QUIC_STATUS_ALPN_NEG_FAILURE (0x80410007)` before the control-stream round-trip completed.
- The checked-in wrapper scripts currently do not provide a fully reliable replacement yet in the shared shell used here: `pwsh -File "c:/Users/heida/source/HoloDesk/scripts/host-transport-build.ps1"` and `pwsh -File "c:/Users/heida/source/HoloDesk/scripts/host-transport-test.ps1"` both failed because `cargo` was not on PATH in that launched PowerShell environment.

## Validation Result

Validation is partial and Milestone 1 remains incomplete. The host runtime implementation is materially further along than Phase 001.1, current editor diagnostics are clean, and the code now reaches real MsQuic runtime behavior instead of transcript-only placeholders. Live localhost smoke validation has not passed yet because the current client/server run fails during handshake with `QUIC_STATUS_ALPN_NEG_FAILURE`, and the new wrapper scripts still need deterministic Cargo discovery in the launched shell.

## Deviations From Plan

Added checked-in PowerShell wrapper scripts plus workflow-instruction updates that were not part of the original technical runtime plan. This deviation was necessary because the session and user workflow required approval-friendly commands and explicitly rejected repeated long inline PowerShell invocations and `cargo run`-based validation.

## Defaults Assumed During Execution

- Local validation remains scoped to `127.0.0.1:4433`.
- `VCPKG_ROOT` remains `C:\Users\heida\vcpkg` for native MsQuic discovery.
- The server certificate source remains a Windows certificate-store SHA-1 thumbprint from `CurrentUser\My` unless the machine-store flag is explicitly enabled.
- The smoke client may use insecure certificate validation only for local localhost validation.

## Blockers or Missing Answers

- The current live handshake still fails with `QUIC_STATUS_ALPN_NEG_FAILURE (0x80410007)`, so the control-stream `hello` -> `hello_ack` round-trip is not yet validated.
- The checked-in build/test wrappers currently assume `cargo` is visible on PATH in the launched PowerShell process, which is not true in the shell used for the latest wrapper-based validation attempt.
- Apple-side runtime validation is still unavailable from this Windows workspace.

## Recommended Next Action

Repair the validation path before attempting more milestone sign-off work:

1. Make the wrapper scripts invoke Cargo through a deterministic toolchain path or a checked-in bootstrap step instead of assuming `cargo` is already on PATH.
2. Fix the server/client handshake mismatch causing `QUIC_STATUS_ALPN_NEG_FAILURE` and rerun both client-initiated and server-initiated close scenarios.
3. After one live localhost round-trip passes, update `docs/Status.md`, `host/transport/README.md`, and `docs/transport-smoke-test.md` with the actual results.