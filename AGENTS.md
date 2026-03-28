# AGENTS.md – HoloBridge Autonomous Agent Guidance

This file is the primary reference for any autonomous coding agent (Copilot, continue-until-blocked, or similar) working in this repository. Read this file before taking any action.

---

## Mission and Scope

Build **HoloBridge v1**: a low-latency Windows desktop and 2D game streaming platform.

- **Host**: Windows application that captures the desktop, encodes video, manages sessions, and streams over QUIC.
- **Client**: Apple Vision Pro native application that authenticates with Apple ID, connects over QUIC, decodes video, and displays a high-quality virtual display.
- **Protocol**: Custom native protocol over HTTP/3 + QUIC. No RTP/RTSP. No WebRTC. No Moonlight/GameStream compatibility.
- **v1 is not a PCVR product.** It is a remote desktop / 2D game streaming product displayed on AVP as a flat virtual display.

---

## Source-of-Truth File Order

When information conflicts, prefer sources in this order:

1. `docs/adr/` – Architecture Decision Records (binding decisions)
2. `docs/streaming-v1.md` – canonical architecture spec for v1
3. `docs/Plan.md` – milestone plan and acceptance criteria
4. `AGENTS.md` – this file (agent workflow and rules)
5. `.github/copilot-instructions.md` – Copilot task-level guidance
6. `.github/instructions/` – per-directory coding instructions
7. Code – implementation

---

## Bootstrap Contract

**Before doing any feature work**, verify that all required bootstrap files exist. If any are missing, create them first:

| File | Purpose |
|---|---|
| `AGENTS.md` | This file |
| `.github/copilot-instructions.md` | Repo-wide Copilot guidance |
| `docs/streaming-v1.md` | Architecture spec |
| `docs/Plan.md` | Milestone plan |
| `docs/Status.md` | Live status tracker |
| `docs/adr/` (at least one ADR) | Architecture decisions |

Do not proceed with feature implementation until all bootstrap files exist and are consistent.

---

## Milestone Workflow

1. Read `AGENTS.md`, `docs/streaming-v1.md`, `docs/Plan.md`, and all files in `docs/adr/`.
2. Read `docs/Status.md` to determine the current state.
3. Identify the **next incomplete milestone** in `docs/Plan.md`.
4. Implement the **smallest end-to-end vertical slice** that satisfies the milestone's acceptance criteria.
5. Run the milestone's validation steps as defined in `docs/Plan.md`.
6. Repair any failures before marking the milestone complete.
7. Update `docs/Status.md` with the completed milestone, changes made, validation results, and next step.
8. **Continue to the next milestone without stopping to ask for permission**, unless a genuine stop condition is reached.

### Stop Conditions

Only stop and request human input when:
- Real ambiguity in requirements that cannot be resolved from the docs.
- Missing secrets, credentials, signing assets, or platform entitlements that cannot be synthesized.
- Repeated (3+) validation failures on the same milestone that require architectural guidance.
- An explicit `BLOCKED` marker is placed in `docs/Status.md` by a human.

**Do not stop just to ask whether to continue to the next planned phase.**

---

## Architecture Rules

These rules are binding and must not be violated without a new ADR:

| Rule | Detail |
|---|---|
| Transport | HTTP/3 + QUIC only. Do not implement RTP/RTSP. |
| Capture (primary) | DXGI Desktop Duplication API |
| Capture (secondary) | Windows.Graphics.Capture – optional, add only after DXGI path is solid |
| Codec | H.264 first. HEVC only after H.264 base path is complete and tested. |
| Auth | Sign in with Apple on AVP client. |
| Host token validation | Host validates Apple identity token (JWT). Maps Apple `sub` to local authorization. |
| Session auth | QUIC session is the authorized active stream context. Do not attach auth to every media packet. |
| Host token | Only a short-lived stream-specific resume token. No broad long-lived host auth tokens in v1. |
| Apple access tokens | Do not use Apple access tokens as host API tokens. |
| Latency | Optimize for low latency and responsiveness over perfect image retention. |
| GPU path | Keep frames on GPU where practical. Avoid unnecessary CPU round-trips. |

---

## Auth Model

1. **Sign in with Apple** – AVP client obtains an Apple identity token (JWT).
2. **Host validates** the identity token against Apple's public keys. Maps the `sub` claim to a locally authorized user record.
3. **QUIC session** – Once authenticated, the QUIC connection is the active stream context. No per-packet auth headers.
4. **Optional App Attest** – extra trust signal for the genuine AVP app/device. Not mandatory in v1.
5. **Resume token** – If the QUIC session is interrupted, the host may issue a short-lived stream-specific resume token. This token is only valid to resume the same stream for a short window (e.g., 60 seconds).
6. **No broad tokens** – Do not mint broad long-lived host auth tokens in v1.

---

## Engineering Style

- Prefer thin vertical slices over broad framework work.
- Separate concerns: capture, encode, transport, auth, input, and session management must not be coupled in the same module.
- Prefer interfaces/protocol boundaries over concrete coupling between layers.
- Prefer placeholders and clear TODO markers over guessing wrong implementation details.
- Do not invent fake build commands, fake Apple configuration values, or fake secrets.
- If a real implementation detail is unknown, document it with a TODO and create a clean interface boundary.
- Keep the GPU path efficient. Avoid CPU round-trips in the hot encode/transport path.

---

## Things to Avoid

- **RTP/RTSP** – not in scope for v1.
- **WebRTC / browser support** – not in scope for v1.
- **Moonlight / GameStream compatibility** – not in scope for v1.
- **PCVR / immersive XR features** – not in scope for v1. AVP is a virtual flat display target.
- **True foveated rendering** – not in scope for v1.
- **Generic OAuth platform** – use Sign in with Apple only.
- **Per-packet authorization** – session-level auth only.
- **Broad long-lived host tokens** – resume token only, stream-scoped, short-lived.
- **Mandatory per-device approval** – not in scope for v1.
- **HEVC before H.264 is solid** – H.264 first.
- **Windows.Graphics.Capture before DXGI works** – DXGI first.
- **Speculative abstractions** – build what the current milestone needs.

---

## docs/Status.md Update Requirement

After completing each milestone, update `docs/Status.md` with:
- Current milestone (move to next)
- The just-completed milestone (add to completed list)
- Summary of changes made
- Validation results
- Any known limitations introduced
- Next recommended step
- Any blockers

Do not skip this step. It is how progress is tracked across sessions.

---

## First Happy Path

The primary user journey all docs and implementations must support:

1. AVP client signs in with Apple.
2. AVP connects to host over QUIC/HTTP3.
3. Host validates Apple identity token and authorizes user.
4. Host creates desktop stream.
5. Host captures desktop via DXGI.
6. Host encodes H.264.
7. Host streams video over QUIC datagrams.
8. AVP decodes and displays video.
9. Input flows back to the host.
10. Reconnect works with a short-lived stream resume token.
