# ADR 0003 – Use quinn Instead of MsQuic for Host QUIC Transport

**Status:** Accepted
**Date:** 2026-04
**Deciders:** HoloBridge architecture team
**Supersedes:** MsQuic recommendation in ADR 0001 Consequences section

---

## Context

ADR 0001 established HTTP/3 + QUIC as the transport protocol and recommended MsQuic as the Windows host QUIC library. During Milestone 1 implementation (phases 001, 001.1, 001.2), repeated attempts to get a working MsQuic-based transport failed due to:

1. **Native dependency management**: MsQuic requires a pre-built native DLL discovered via vcpkg. The `msquic` Rust crate's `find` feature depends on `VCPKG_ROOT` being set and the correct native library version being installed.

2. **Version mismatch**: The Rust `msquic` crate (v2.5.1-beta) generated FFI bindings for MsQuic 2.5.x, but the vcpkg-installed native DLL was MsQuic 2.4.8. This ABI mismatch caused `QUIC_STATUS_ALPN_NEG_FAILURE` during TLS handshake — the bindings and native library disagreed on struct layouts or function signatures.

3. **Unsafe FFI complexity**: The MsQuic Rust crate is a thin wrapper over a C API that uses callbacks, raw pointers, and manual lifetime management. This required `unsafe` blocks for connection ownership, stream callback installation, send buffer management, and handle cleanup. The callback-driven model also required shared state behind `Arc<Mutex<>>` for inter-callback communication.

4. **Windows-specific certificate management**: MsQuic on Windows uses Schannel, requiring certificates to be provisioned in the Windows certificate store with SHA-1 thumbprint lookup via `CertificateHash` or `CertificateHashStore` APIs.

5. **Experimental Rust support**: The MsQuic Rust crate is explicitly labeled as experimental, with limited documentation and a small user base.

Three corrective phases (001, 001.1, 001.2) were unable to achieve a single successful `hello` → `hello_ack` round-trip over localhost despite the code compiling and the MsQuic listener starting.

---

## Decision

Replace the `msquic` crate with **quinn** — a pure Rust, async/await QUIC implementation built on **rustls** for TLS.

---

## Rationale

### Why quinn

| Property | MsQuic (Rust crate) | quinn |
|---|---|---|
| Language | C with Rust FFI bindings | Pure Rust |
| Native dependencies | vcpkg + msquic.dll + VCPKG_ROOT | None |
| TLS provider | Windows Schannel (platform-specific) | rustls (cross-platform, pure Rust) |
| Certificate management | Windows certificate store (SHA-1 thumbprint) | In-memory via rcgen (self-signed) or PEM files |
| API model | Callback-driven, unsafe, raw pointers | Async/await, safe Rust, futures |
| Unsafe code required | Yes (connection ownership, buffer lifetime, callbacks) | No |
| QUIC datagrams | Supported | Supported |
| Maturity | Experimental Rust bindings | Stable, widely used (86M+ downloads) |
| Cross-platform | Windows-only (Schannel path) | Windows, macOS, Linux |

### What changed in the codebase

- `server.rs`: Complete rewrite from ~950 lines of callback/mutex/condvar code to ~330 lines of async/await code.
- `tls.rs`: Replaced Windows certificate store lookup with rcgen self-signed cert generation + rustls config builders.
- `config.rs`: Removed `CertificateSource::WindowsCertificateHash` (SHA-1 thumbprint, store name, machine store flag). Replaced with `CertificateSource::SelfSigned`.
- `Cargo.toml`: Replaced `msquic` + vcpkg dependency with `quinn`, `rustls`, `rcgen`, `tokio`.
- No changes to `protocol.rs` (framing), `connection.rs` (state machine), or `tests/codec_roundtrip.rs`.

### What we keep from ADR 0001

The core transport decision (QUIC + unreliable datagrams for video, reliable streams for control) is unchanged. Only the implementation library changed. quinn supports all the same QUIC features required by the architecture:

- Custom ALPN negotiation
- Bidirectional streams for control messages
- QUIC datagrams for future video transport (RFC 9221)
- TLS 1.3 built into the QUIC connection

---

## Consequences

- The host transport no longer requires vcpkg, `VCPKG_ROOT`, or a pre-installed MsQuic native DLL.
- The host transport no longer requires a Windows certificate store certificate or SHA-1 thumbprint configuration.
- The host transport now requires the tokio async runtime.
- The AVP client still uses `Network.framework` for its QUIC transport (Apple-native, not affected by this decision).
- Future milestones that use QUIC datagrams will use quinn's datagram API instead of MsQuic's.
- The MsQuic recommendation in ADR 0001's Consequences section is superseded by this ADR.

---

## Alternatives Considered

### Fix the MsQuic version mismatch

Upgrading vcpkg's MsQuic to 2.5.x was considered but vcpkg only ships MsQuic 2.4.8. Building MsQuic from source via the crate's `src` feature failed due to missing XDP headers. Even if the version mismatch were fixed, the unsafe FFI complexity, callback-driven architecture, and Windows-specific certificate management would remain as ongoing maintenance burdens.

### quiche (Cloudflare)

quiche is a C QUIC implementation with Rust bindings. Rejected for the same FFI complexity reasons as MsQuic — it would trade one set of native binding issues for another.

### s2n-quic (AWS)

s2n-quic is a Rust QUIC implementation but has a smaller community than quinn and less documentation. quinn was preferred for its maturity and ecosystem.
