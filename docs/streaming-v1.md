# HoloBridge v1 – Streaming Architecture

**Version:** v1  
**Status:** Design complete, implementation in progress  
**Last Updated:** Milestone 0

---

## Product Goal

Enable low-latency remote access to a Windows desktop and 2D games, displayed on Apple Vision Pro as a high-quality virtual flat display.

This is a **remote display product**, not a PCVR product. The Apple Vision Pro renders a 2D stream window; it does not render 3D content from the Windows host.

---

## Scope

**In scope for v1:**
- Remote Windows desktop capture and streaming
- Remote 2D game streaming (full-screen or windowed)
- Apple Vision Pro as a flat virtual display client
- Custom native protocol over HTTP/3 + QUIC
- Sign in with Apple authentication
- H.264 video codec
- Pointer, keyboard, and scroll input forwarding
- Session reconnect with short-lived resume token

**Non-goals for v1:**
- Browser / WebRTC client
- Moonlight / GameStream compatibility
- PCVR / immersive XR features (spatial rendering, passthrough blending, etc.)
- True foveated rendering
- Generic OAuth platform
- Mandatory per-device approval
- HEVC codec (may be added after H.264 is solid)
- Windows.Graphics.Capture (may be added after DXGI is solid)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                     Windows Host                         │
│                                                         │
│  ┌─────────┐   ┌─────────┐   ┌────────────────────┐    │
│  │ Capture │──▶│ Encode  │──▶│     Transport      │    │
│  │  DXGI   │   │  H.264  │   │  QUIC datagrams    │    │
│  └─────────┘   └─────────┘   └────────────────────┘    │
│                                        ▲                │
│  ┌─────────┐   ┌─────────────────┐     │                │
│  │  Input  │   │  Auth / Session │─────┘                │
│  │ Replay  │   │  QUIC streams   │                      │
│  └─────────┘   └─────────────────┘                      │
└─────────────────────────────────────────────────────────┘
                          │ QUIC / HTTP3
                          ▼
┌─────────────────────────────────────────────────────────┐
│               Apple Vision Pro Client                   │
│                                                         │
│  ┌─────────┐   ┌─────────┐   ┌────────────────────┐    │
│  │Transport│──▶│ Decode  │──▶│      Display       │    │
│  │  QUIC   │   │VideoTB  │   │  Metal / SwiftUI   │    │
│  └─────────┘   └─────────┘   └────────────────────┘    │
│                                                         │
│  ┌─────────┐   ┌─────────────────┐                      │
│  │  Input  │   │  Auth / Session │                      │
│  │ Capture │   │  Sign in Apple  │                      │
│  └─────────┘   └─────────────────┘                      │
└─────────────────────────────────────────────────────────┘
```

---

## Host Responsibilities

| Responsibility | Detail |
|---|---|
| Desktop capture | DXGI Desktop Duplication (`IDXGIOutputDuplication`) on the target display |
| Encode | Hardware H.264 encoder (Media Foundation / NVENC / QuickSync / AMF), low-latency preset |
| Video transport | QUIC datagrams – one or more datagrams per encoded frame |
| Control transport | QUIC streams – session setup, auth, input, resume token exchange |
| Auth validation | Validate Apple identity token JWT against Apple's public keys; map `sub` to local user |
| Session management | Create, hold, resume, and terminate stream sessions |
| Resume token | Issue short-lived stream-scoped token on session interruption |
| Input replay | Receive pointer/keyboard/scroll from client; replay via `SendInput` or equivalent |

---

## AVP Client Responsibilities

| Responsibility | Detail |
|---|---|
| Sign in with Apple | Obtain identity token using `ASAuthorizationAppleIDProvider` |
| QUIC connection | Establish HTTP/3 + QUIC connection to host |
| Session setup | Send identity token over QUIC control stream; receive session confirmation |
| Video receive | Receive H.264 NALU data over QUIC datagrams |
| Decode | VideoToolbox hardware H.264 decoder; keep frames on GPU (Metal textures) |
| Display | Present decoded frames as a flat virtual display window in AVP environment |
| Input capture | Capture pointer/gaze, keyboard, scroll from visionOS; forward over QUIC control stream |
| Reconnect | Use host-issued resume token to reconnect after QUIC interruption |

---

## Auth / AuthZ Model

### Sign in with Apple

The AVP client uses `ASAuthorizationAppleIDProvider` to sign in. This produces:
- **Identity token** (JWT, short-lived): contains `sub` (stable user identifier), `iss`, `aud`, `exp`, and optionally `email`.
- **Authorization code** (one-time): used for server-side token exchange if needed.

In v1, only the identity token is required. The authorization code is not used.

### Host Token Validation

1. Client sends identity token to host over the QUIC control stream.
2. Host fetches Apple's JSON Web Key Set from `https://appleid.apple.com/auth/keys`.
3. Host validates the JWT signature, `iss` (`https://appleid.apple.com`), `aud` (app's bundle ID), and `exp`.
4. Host extracts `sub` and maps it to a locally authorized user record.
5. If authorization succeeds, the QUIC session is now the active auth context.

### Active Session Auth

- The QUIC connection is the authorized session. No per-packet auth tokens.
- No broad long-lived host tokens are minted.

### Optional App Attest

Apple's App Attest API can be used as an additional trust signal to verify the client is the genuine AVP app running on a real device. This is optional in v1 and should be treated as defense-in-depth. It does not replace identity token validation.

### Resume Token

- When a QUIC session is interrupted, the host may issue a **stream-specific resume token**.
- The token is short-lived (suggested: 60 seconds).
- The token is scoped to one specific stream session.
- The client presents the token when reconnecting to resume the same stream.
- The token does not carry any authorization beyond resuming that one stream.
- Invalidate the token after successful resume or expiry.

---

## Transport Model

### Protocol Stack

```
Application (HoloBridge control + media)
      │
HTTP/3 (QUIC streams for control; QUIC datagrams for media)
      │
QUIC (UDP-based, multiplexed, encrypted)
      │
UDP / IP
```

### QUIC Usage

| Channel | Mechanism | Purpose |
|---|---|---|
| Control | QUIC bidirectional stream | Session setup, auth, input events, control messages |
| Video | QUIC unreliable datagrams | Video frame delivery (low latency, no HOL blocking) |
| Resume | QUIC control stream | Resume token exchange |

### Why QUIC Datagrams for Video

QUIC datagrams are unreliable (like UDP) but encrypted and authenticated by the QUIC session. Dropped frames are acceptable for low-latency video; retransmission would add unacceptable latency. The encoder produces independently-decodable frames (IDR/keyframes and P-frames with periodic IDR refresh) so that a lost frame results in a brief artifact, not a stream stall.

---

## Session Lifecycle

```
Client                                   Host
  │                                        │
  │──── Sign in with Apple ──────────────▶ │
  │                                        │
  │──── QUIC connect ─────────────────────▶│
  │──── STREAM: identity token ───────────▶│
  │                                ┌───────┤
  │                                │ Validate token
  │                                │ Map sub → user
  │                                └───────┤
  │◀─── STREAM: session confirmed ─────────│
  │                                        │
  │◀═══ DATAGRAMS: H.264 video ════════════│
  │──── STREAM: input events ─────────────▶│
  │                                        │
  │  [QUIC interrupted]                    │
  │                                ┌───────┤
  │                                │ Issue resume token
  │                                └───────┤
  │◀─── STREAM: resume token ──────────────│
  │                                        │
  │──── QUIC reconnect ───────────────────▶│
  │──── STREAM: resume token ─────────────▶│
  │◀─── STREAM: session resumed ───────────│
  │◀═══ DATAGRAMS: H.264 video ════════════│
  │                                        │
  │──── STREAM: disconnect ───────────────▶│
  │                                        │
```

---

## Resume Token Design

| Property | Value |
|---|---|
| Issuer | Host |
| Scope | Single stream session |
| Lifetime | Short-lived (suggested: 60 seconds) |
| Format | Opaque token (e.g., HMAC-SHA256 over session ID + expiry) |
| Validation | Host checks token signature and expiry; invalidates after first successful use |
| What it does NOT do | Does not authorize new streams, does not carry user identity claims |

---

## Capture Strategy

### Primary: DXGI Desktop Duplication

- API: `IDXGIOutput1::DuplicateOutput` → `IDXGIOutputDuplication`
- Frame acquisition: `AcquireNextFrame` produces a `DXGI_OUTDUPL_FRAME_INFO` and an `ID3D11Texture2D` in GPU memory.
- Zero-copy path: pass the `ID3D11Texture2D` directly to the encoder's input surface.
- Frame rate: match the display refresh or cap at target streaming frame rate (e.g., 60 fps).

### Secondary: Windows.Graphics.Capture (optional, not in v1)

- Captures individual windows or the entire desktop.
- Lower system impact on some configurations.
- Add only after DXGI path is solid and the secondary path has a clear use case.

---

## Encode / Decode Strategy

### Host Encode

- Codec: H.264 (AVC), Baseline or Main profile.
- API: Windows Media Foundation (`IMFTransform`) or hardware vendor SDK (NVENC, Intel QuickSync, AMD AMF).
- Preset: low-latency (disable B-frames, minimize DPB, set `MaxFrameLatency = 1`).
- IDR refresh: periodic (e.g., every 2 seconds) to limit artifact duration on packet loss.
- Output: Annex-B NALU stream, sliced for QUIC datagram delivery.

### AVP Decode

- API: VideoToolbox (`VTDecompressionSession`).
- Input: H.264 Annex-B or AVCC format NALUs.
- Output: `CVPixelBuffer` / Metal texture.
- Keep decoded frames on GPU. Avoid CPU round-trips in the display path.

---

## Telemetry

Track the following per-session metrics (v1 minimum):
- Frame capture latency (capture → encode input)
- Encode latency (encode input → NALU ready)
- Transport latency (NALU ready → datagram sent; approximate RTT via QUIC stats)
- Decode latency (datagram received → frame decoded)
- Display latency (frame decoded → frame displayed)
- Frame drop rate (datagrams lost)
- Reconnect count and resume token usage

Telemetry is for diagnostics and optimization. Do not send telemetry to external services in v1 without explicit configuration.

---

## Reliability and Recovery Priorities

1. **Low latency is the primary goal.** Accept occasional frame drops over latency spikes.
2. **IDR refresh** ensures recovery from packet loss within a bounded time window.
3. **QUIC reconnect with resume token** handles short network interruptions.
4. **Session termination** on extended interruption (resume token expired). Client must re-authenticate.
5. **No forward error correction (FEC)** in v1. May be evaluated in a later milestone.
