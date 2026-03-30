# HoloDesk

Low-latency Windows desktop and 2D game streaming to Apple Vision Pro.

HoloBridge is the streaming platform inside this repository. v1 targets remote Windows desktop access and remote 2D game streaming displayed on Apple Vision Pro as a high-quality virtual display, using a custom native protocol over HTTP/3 + QUIC.

## Repo Layout

```
host/               Windows capture, encode, transport, auth, and session host
client-avp/         Apple Vision Pro native client (SwiftUI / RealityKit display layer)
docs/               Architecture specs, planning, and status
docs/adr/           Architecture Decision Records
.github/            GitHub configuration
.github/agents/     Custom Copilot agent definitions
.github/instructions/ Per-directory coding instructions for Copilot
AGENTS.md           Autonomous agent guidance and milestone workflow
```

## Current Status

**Milestone 0 complete** – repo scaffolding, documentation, and agent setup done.

See [docs/Status.md](docs/Status.md) for the live project status.

## Key Documents

- [docs/streaming-v1.md](docs/streaming-v1.md) – full architecture and design spec for v1
- [docs/Plan.md](docs/Plan.md) – milestone-by-milestone implementation plan
- [docs/Status.md](docs/Status.md) – current milestone status and known blockers
- [docs/adr/](docs/adr/) – Architecture Decision Records

## Quick Architecture Summary

| Concern | Decision |
|---|---|
| Transport | HTTP/3 + QUIC (custom native protocol) |
| Capture | DXGI Desktop Duplication (primary), Windows.Graphics.Capture (secondary) |
| Codec | H.264 first; HEVC optional after base path is solid |
| Auth | Sign in with Apple on AVP client; host validates Apple identity token |
| Active session auth | QUIC session context (not per-packet tokens) |
| Resume | Short-lived stream-specific resume token only |
| PCVR / XR | Not in scope for v1 |
| GameStream / Moonlight | Not in scope for v1 |

## Getting Started

> **Status: pre-alpha.** The implementation milestones have not yet started.

See [docs/Plan.md](docs/Plan.md) for the ordered milestone plan and [AGENTS.md](AGENTS.md) for guidance on autonomous development workflow.