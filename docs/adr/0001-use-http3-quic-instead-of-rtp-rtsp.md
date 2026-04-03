# ADR 0001 – Use HTTP/3 + QUIC Instead of RTP/RTSP

**Status:** Accepted  
**Date:** 2026-03  
**Deciders:** HoloBridge architecture team

---

## Context

HoloBridge v1 needs a transport protocol for streaming H.264 video frames from a Windows host to an Apple Vision Pro client over a local or remote network. The two primary candidates evaluated were:

1. **RTP/RTSP** – the traditional real-time media transport stack used by most streaming systems (including Moonlight/GameStream).
2. **HTTP/3 + QUIC** – the modern transport stack built on QUIC, a UDP-based multiplexed transport with built-in encryption.

---

## Decision

HoloBridge v1 will use **HTTP/3 + QUIC** as its transport layer. RTP and RTSP will not be implemented.

---

## Rationale

### Why QUIC is preferred

| Property | RTP/RTSP | HTTP/3 + QUIC |
|---|---|---|
| Encryption | Optional (SRTP needed separately) | Built-in (TLS 1.3 in QUIC) |
| Connection setup | Multi-step (RTSP handshake) | 0-RTT or 1-RTT (QUIC) |
| Multiplexing | Separate RTP and RTCP channels | Single QUIC connection, multiple streams |
| Head-of-line blocking | Present on TCP fallback (RTSPS) | None – QUIC streams are independent |
| Unreliable datagrams | Requires raw UDP; no QUIC-session protection | QUIC datagrams (unreliable, but QUIC-authenticated) |
| Firewall traversal | Often blocked (non-HTTP port) | HTTP/3 on standard port (443), UDP |
| Control and media multiplexing | Requires separate TCP/UDP sockets | Single QUIC connection for both |
| Future extensibility | Limited; protocol is largely frozen | Active IETF standard with extensions (e.g., WebTransport) |

### Why QUIC datagrams for video

Video frames must be delivered with minimal latency. TCP-based transport (or QUIC streams) would introduce head-of-line blocking when a packet is lost: the transport layer would wait for retransmission before delivering subsequent frames. For video, it is better to drop a frame than to stall the stream.

QUIC unreliable datagrams provide:
- Low latency delivery (no retransmission blocking)
- Encryption and authentication by the QUIC session (unlike raw UDP)
- No separate socket or channel management

The H.264 encoder is configured with periodic IDR refresh, so a dropped frame results in a brief visual artifact, not a stream stall.

### Why HTTP/3 control streams

Control messages (session setup, auth, input events, resume token exchange) require reliable ordered delivery. QUIC streams provide exactly this, with the latency benefits of QUIC connection establishment and no head-of-line blocking between the control stream and the media datagram channel.

### Why not RTP/RTSP

- RTP/RTSP is a legacy protocol stack primarily maintained for compatibility with existing infrastructure (e.g., IP cameras, media servers).
- HoloBridge v1 has no compatibility requirement with any existing streaming infrastructure.
- RTP/RTSP does not provide built-in encryption; SRTP and RTSPS add complexity.
- RTSP is a stateful session protocol over TCP; adding a UDP media channel alongside it adds socket management complexity.
- The Moonlight/GameStream protocol (which uses RTSP) is explicitly not a v1 compatibility target.
- QUIC provides a strictly superior set of properties for a new custom protocol.

---

## Consequences

- The host must use a QUIC library that supports unreliable datagrams and QUIC streams. The host uses **quinn** (pure Rust). See [ADR 0003](0003-use-quinn-instead-of-msquic.md) for the rationale behind choosing quinn over the originally considered MsQuic.
- The AVP client must use a QUIC client. On Apple platforms, **`Network.framework`** supports QUIC natively (available on iOS 15+, visionOS 1+).
- A new ADR should be filed if a specific QUIC library is chosen that requires justification.
- RTP and RTSP must not be introduced in v1 without a superseding ADR.

---

## Alternatives Considered

### WebRTC

WebRTC was considered but rejected because:
- Browser/WebRTC support is explicitly a non-goal for v1.
- WebRTC adds significant complexity (ICE, STUN/TURN, SDP negotiation, DTLS).
- WebRTC's media pipeline is opinionated and difficult to optimize for custom low-latency paths.
- A native QUIC stack gives more direct control over the transport.

### Raw UDP

Raw UDP was considered for the media path but rejected because:
- It provides no encryption or authentication.
- It requires implementing session-level security separately.
- QUIC datagrams provide the same latency properties with built-in session security.
