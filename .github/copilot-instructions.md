# Copilot Instructions – HoloBridge

These instructions apply to all Copilot tasks in this repository. Read them before completing any task.

---

## Bootstrap Check

Before doing any feature work, verify these files exist. If any are missing, create them first:

- `AGENTS.md`
- `.github/copilot-instructions.md` (this file)
- `docs/streaming-v1.md`
- `docs/Plan.md`
- `docs/Status.md`
- `docs/adr/` with at least one ADR

Do not skip this check, even if the immediate task seems unrelated.

---

## Project Summary

HoloBridge v1 is a low-latency Windows desktop and 2D game streaming platform targeting Apple Vision Pro as a virtual flat display. It is **not** a PCVR product.

- **Host**: Windows app – capture, encode, transport, session management.
- **Client**: AVP native app – auth, QUIC connection, decode, display.
- **Protocol**: Custom native protocol over HTTP/3 + QUIC.

---

## Architecture Defaults

Apply these defaults to all code unless an ADR explicitly overrides them:

| Concern | Default |
|---|---|
| Transport | HTTP/3 + QUIC |
| Capture | DXGI Desktop Duplication (primary) |
| Codec | H.264 |
| Client auth | Sign in with Apple |
| Host auth | Validate Apple identity token (JWT) server-side |
| Session auth | QUIC session context – no per-packet tokens |
| Host token | Short-lived stream-specific resume token only |

---

## Auth Defaults

- AVP client uses **Sign in with Apple** to obtain an identity token.
- Host validates the identity token against Apple's public keys.
- Host maps the Apple `sub` claim to a local authorized user.
- The QUIC connection is the active authorized session context.
- Do **not** use Apple access tokens as host API tokens.
- Do **not** mint broad long-lived host auth tokens.
- The only host-issued token is a short-lived, stream-specific resume token for reconnection.

---

## Anti-Goals for v1

Do not implement, suggest, or scaffold these in v1:

- RTP / RTSP
- WebRTC / browser client
- Moonlight / GameStream compatibility
- PCVR / immersive XR features
- True foveated rendering
- Generic OAuth platform
- Mandatory per-device approval
- HEVC before H.264 is complete
- Windows.Graphics.Capture before DXGI is working

---

## Coding Style

- Separate concerns: capture, encode, transport, auth, input, and session must not be coupled.
- Prefer interfaces/protocol boundaries over concrete coupling across layers.
- Keep frames on the GPU where practical.
- Optimize for low latency, not perfect image retention.
- Use placeholders and clear TODO markers instead of guessing wrong implementation details.
- Do not invent fake secrets, fake Apple configuration, or fake build commands.

---

## Milestone Progression

- Do **not** pause after completing a phase to ask whether to continue.
- Continue milestone by milestone until a genuine stop condition is reached.
- Genuine stop conditions: missing secrets/credentials/signing assets, real architectural ambiguity, or repeated validation failures.
- After each milestone, update `docs/Status.md`.

See `AGENTS.md` for the full milestone workflow.
