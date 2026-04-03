# HoloBridge Host

**Rust** Windows application: desktop capture, H.264 encoding, QUIC/HTTP3 transport, authentication, session management, and input replay.

## Status

**Pre-implementation.** Directory structure is scaffolded. Implementation begins at Milestone 1.

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

## TODO (Milestone 1)

- [x] Select QUIC library (quinn — pure Rust, see ADR 0003)
- [x] Scaffold QUIC server in `transport/`
- [x] Implement minimal control stream message exchange
- [x] Verify loopback connectivity with client
