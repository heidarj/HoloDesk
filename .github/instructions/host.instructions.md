# Host Instructions

These instructions apply to all files under `host/`.

---

## Role

The host is a Windows application responsible for:
- Capturing the desktop (DXGI Desktop Duplication primary, Windows.Graphics.Capture secondary)
- Encoding video (H.264 primary, HEVC secondary)
- Managing QUIC/HTTP3 transport
- Validating Apple identity tokens and authorizing users
- Managing stream sessions and issuing resume tokens
- Receiving and replaying input events from the client

---

## Concern Separation

Keep these concerns in separate modules. Do not mix them:

| Concern | Module area |
|---|---|
| Capture | `host/capture/` – DXGI and optional WGC backends |
| Encode | `host/encode/` – H.264 encoder, codec pipeline |
| Transport | `host/transport/` – QUIC/HTTP3 session, stream, datagram management |
| Auth | `host/auth/` – Apple identity token validation, user authorization, resume token |
| Input | `host/input/` – receive input events from client, replay via Win32 |
| Session | `host/session/` – stream lifecycle, session state, resume token issuance |

---

## Capture

- **DXGI Desktop Duplication** is the default and primary capture backend.
- `IDXGIOutputDuplication::AcquireNextFrame` is the expected API.
- Keep captured frames on the GPU (as `ID3D11Texture2D`). Avoid CPU round-trips.
- `Windows.Graphics.Capture` is a secondary/optional backend. Do not add it until the DXGI path is solid and tested.
- Expose a common `ICaptureBackend` interface so the encode stage is decoupled from capture implementation.

---

## Encode

- H.264 is the required codec for v1. Use Media Foundation or a hardware-accelerated encoder (NVENC, QuickSync, AMF).
- HEVC may be added only after H.264 is complete, tested, and validated end-to-end.
- Keep the encode pipeline on the GPU where possible (zero-copy from capture to encoder input).
- Optimize for low-latency encoder presets. Latency takes priority over compression efficiency.
- Expose a common `IVideoEncoder` interface so transport is decoupled from codec details.

---

## Transport

- HTTP/3 + QUIC only. Do not introduce RTP, RTSP, or WebRTC.
- Use QUIC datagrams for video frame delivery (low latency, no head-of-line blocking).
- Use QUIC streams for control messages (session setup, auth, input, resume token exchange).
- The QUIC connection/session is the active authorized stream context.
- Do not attach auth tokens to individual media packets.

---

## Auth

- Accept the Apple identity token (JWT) from the client at session setup.
- Validate the token against Apple's public keys (`https://appleid.apple.com/auth/keys`).
- Extract the `sub` claim and map it to a locally authorized user record.
- Do not use Apple access tokens as host API tokens.
- After validation, the QUIC session is the auth context. No further per-packet auth.
- Issue a short-lived (e.g., 60-second) stream-specific resume token if the session is interrupted.
- Do not mint broad long-lived host auth tokens in v1.

---

## Input

- Receive pointer, keyboard, and scroll events from the AVP client over the QUIC control stream.
- Replay events using `SendInput` (Win32) or equivalent low-level API.
- Keep input handling decoupled from session management.

---

## Session / Stream Lifecycle

- Session creation: validate auth, set up capture and encode pipelines, begin streaming.
- Session active: stream video datagrams; handle control messages.
- Session interrupted: issue resume token; hold state briefly.
- Session resumed: validate resume token; resume from held state.
- Session terminated: release capture/encode resources, invalidate resume token.

---

## Performance and Latency

- Minimize end-to-end latency. Prefer lower latency over perfect image retention.
- Keep the capture-encode-transport path on the GPU where practical.
- Avoid blocking calls in the hot path.
- Avoid speculative abstractions. Build what the current milestone needs.
