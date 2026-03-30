# HoloBridge AVP Client

Apple Vision Pro native application: Sign in with Apple, QUIC/HTTP3 transport, H.264 decode via VideoToolbox, flat virtual display, and input forwarding.

## Status

**Pre-implementation.** Directory structure is scaffolded. Implementation begins at Milestone 1.

## Planned Structure

```
client-avp/
├── Auth/           Sign in with Apple, identity token management, resume token storage
├── Transport/      QUIC/HTTP3 client, control streams, video datagrams
├── Decode/         VideoToolbox H.264 decoder pipeline
├── Display/        Metal / SwiftUI flat virtual display rendering
├── Input/          visionOS input capture, encoding, and forwarding
├── Session/        Session lifecycle, reconnection logic
└── App/            SwiftUI App entry point, root views
```

## Key Constraints

- This is a **virtual flat display** client. It is not a PCVR or immersive XR app.
- Sign in with Apple (`ASAuthorizationAppleIDProvider`) is the only auth mechanism.
- HTTP/3 + QUIC only for transport. No WebRTC or RTP.
- VideoToolbox hardware H.264 decoder. Keep frames on GPU (Metal).
- QUIC session is the active auth context. No per-packet auth.
- Resume tokens are short-lived and stream-scoped.

## Platform Requirements

- visionOS 1.0+
- Sign in with Apple entitlement required (Apple Developer account configuration needed)
- Bundle ID and Apple client ID to be configured – **do not invent these values**

See [../docs/streaming-v1.md](../docs/streaming-v1.md) for the full architecture spec.  
See [../.github/instructions/client-avp.instructions.md](../.github/instructions/client-avp.instructions.md) for coding instructions.

## TODO (Milestone 1)

- [ ] Configure visionOS project (Xcode, bundle ID, entitlements)
- [ ] Select QUIC approach (suggested: `Network.framework` QUIC, visionOS 1+)
- [ ] Scaffold QUIC client in `Transport/`
- [ ] Implement minimal control stream message exchange
- [ ] Verify loopback connectivity with host
