# HoloBridge Host

**Rust** Windows application: desktop capture, H.264 encoding, QUIC/HTTP3 transport, authentication, session management, and input replay.

## Status

Milestones 1 through 3 are complete. Milestone 4 is in progress with a new
`capture/` crate that defines the host-side capture interfaces and the DXGI
Desktop Duplication path.

## Planned Structure

```
host/
├── capture/        DXGI Desktop Duplication capture backend
│                   ICaptureBackend interface
├── encode/         H.264 encoder pipeline (MFT / NVENC / QuickSync / AMF)
│                   IVideoEncoder interface
├── transport/      QUIC/HTTP3 server, control streams, video datagrams
├── auth/           Apple identity token validation, user authorization, resume tokens
├── input/          Input event receive and Win32 replay (SendInput)
├── session/        Stream session lifecycle management
└── main/           Entry point and configuration
```

## Key Constraints

- The host is written in **Rust**. Prefer safe Rust; minimize `unsafe`. Use narrow FFI for DXGI/codec interop.
- DXGI Desktop Duplication is the primary capture backend.
- H.264 is the required codec for v1.
- HTTP/3 + QUIC only for transport. No RTP/RTSP.
- Apple identity token validation is required at session creation.
- QUIC session is the active auth context. No per-packet auth.
- Resume tokens are short-lived and stream-scoped.

See [../docs/streaming-v1.md](../docs/streaming-v1.md) for the full architecture spec.  
See [../.github/instructions/host.instructions.md](../.github/instructions/host.instructions.md) for coding instructions.

## Current Focus

- [x] QUIC transport in `transport/`
- [x] Apple auth and local authorization in `auth/`
- [x] Session lifecycle and resume tokens in `session/`
- [ ] DXGI display enumeration and GPU texture capture in `capture/`
