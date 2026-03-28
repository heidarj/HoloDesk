# Client AVP Instructions

These instructions apply to all files under `client-avp/`.

---

## Role

The AVP client is a native Apple Vision Pro application responsible for:
- Authenticating the user with Sign in with Apple
- Connecting to the host over QUIC/HTTP3
- Decoding H.264 video received from the host
- Displaying decoded video as a high-quality flat virtual display
- Capturing and forwarding user input (pointer, keyboard, scroll) to the host
- Handling reconnection using a host-issued resume token

---

## Display Model

- **AVP is a virtual flat display target, not a PCVR or immersive XR target.**
- Display the stream as a high-quality 2D virtual display window in AVP's environment.
- Use `RealityKit` or `SwiftUI` for the display layer as appropriate.
- Do not assume immersive spaces, passthrough blending, or spatial audio requirements unless explicitly added in a later milestone.
- Prefer native visionOS APIs. Do not use ARKit/RealityKit spatial anchoring for the video surface unless it provides a clear latency or quality benefit.

---

## Auth

- Use **Sign in with Apple** (`ASAuthorizationAppleIDProvider`) to obtain an identity token.
- Send the identity token to the host at session setup over the QUIC control stream.
- Do not use Apple access tokens as the host API token.
- Do not store or re-use the Apple access token beyond the sign-in flow.
- After session setup, the QUIC connection is the active auth context.
- Handle resume token receipt, storage (in-memory), and use for reconnection.

---

## Transport

- HTTP/3 + QUIC only. Do not introduce WebRTC, RTP, or RTSP.
- Use a QUIC control stream for session setup, auth, input, and control messages.
- Use QUIC datagrams for receiving video frame data.
- Handle QUIC connection interruptions gracefully: use the resume token to reconnect.

---

## Decode and Display

- Decode H.264 video using `VideoToolbox` hardware decoder.
- Keep decoded frames on the GPU (Metal textures) where possible.
- Display frames with minimal buffering to minimize latency.
- Do not add unnecessary display buffering. Latency is more important than smoothness in v1.
- Expose a clean decode pipeline interface so the display layer is decoupled from codec details.

---

## Input

- Capture pointer/gaze, keyboard (if applicable), and scroll input from visionOS.
- Forward input events to the host over the QUIC control stream.
- Keep input capture and forwarding decoupled from the display/decode layer.

---

## Concern Separation

Keep these concerns in separate modules:

| Concern | Module area |
|---|---|
| Auth | `client-avp/Auth/` – Sign in with Apple, token storage, resume token |
| Transport | `client-avp/Transport/` – QUIC connection, stream, datagram management |
| Decode | `client-avp/Decode/` – VideoToolbox H.264 decoder |
| Display | `client-avp/Display/` – Metal / SwiftUI / RealityKit display layer |
| Input | `client-avp/Input/` – Input capture and event forwarding |
| Session | `client-avp/Session/` – Session lifecycle, reconnection logic |

---

## Things to Avoid

- Immersive XR assumptions (spatial anchors, passthrough blend modes, etc.) – not in scope for v1.
- WebRTC, browser-based, or Moonlight/GameStream compatibility layers.
- HEVC decode path before H.264 is solid.
- Per-packet auth headers – the QUIC session is the auth context.
- Broad long-lived tokens – use only the host-issued short-lived resume token.
- Speculative abstractions for features not in the current milestone.

---

## Coding Style

- Write in Swift using visionOS SDK.
- Use `async/await` and Swift concurrency where appropriate.
- Separate UI concerns from transport and decode concerns.
- Use protocols/interfaces to decouple layers.
- Prefer placeholders and clear TODO markers over guessing unknown implementation details (e.g., specific App Attest configuration, entitlement values).
