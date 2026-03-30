# AGENTS.md

## Mission
Build v1 of HoloBridge:
- `host/` = Rust Windows host/server
- `client-avp/` = Apple Vision Pro native client

This is **not** PCVR in v1.
The target is remote desktop and remote 2D game streaming shown as a high-quality virtual display on AVP.

## Required reading
Before substantial changes, read:
- `docs/streaming-v1.md`
- `docs/Plan.md`
- `docs/adr/`

If bootstrap files are missing, create them before feature work:
- `AGENTS.md`
- `.github/copilot-instructions.md`
- `docs/streaming-v1.md`
- `docs/Plan.md`
- `docs/Status.md`
- `docs/adr/` with at least one ADR

## Workflow
Work milestone-by-milestone using `docs/Plan.md`.

For each milestone:
1. Identify the next incomplete milestone.
2. Implement the smallest end-to-end slice needed to satisfy it.
3. Run the milestone validation steps.
4. Fix failures before moving on.
5. Update `docs/Status.md` with:
   - what changed
   - validation results
   - current limitations
   - next recommended step

Continue automatically to the next milestone unless blocked.

## Stop conditions
Stop and ask for human input only if:
- there is a real product ambiguity that changes architecture or UX
- credentials, secrets, signing assets, Apple developer configuration, or certificates are required
- a milestone fails validation repeatedly after reasonable repair attempts
- external behavior is unclear and cannot be verified safely
- the requested change conflicts with these instructions or ADRs

Do not stop just to ask whether to continue to the next planned phase.

## Core architecture rules
- Use **HTTP/3 + QUIC** for v1.
- Do **not** implement RTP/RTSP unless explicitly requested later.
- Use **Sign in with Apple** on the AVP client.
- The host must validate the **Apple identity token** and map Apple `sub` to local authorization.
- Do **not** use Apple access tokens as host API tokens.
- Do **not** mint a broad long-lived host auth token in v1.
- Treat the **QUIC session** as the authorized active stream context.
- The only host-issued token in v1 should be a **short-lived stream-specific resume token**.
- Do **not** attach authorization to every media packet.
- Prefer **DXGI Desktop Duplication** as the default Windows capture backend.
- Treat **Windows.Graphics.Capture** as an optional secondary backend.
- Prefer **H.264** first. Add **HEVC** only after the base path is solid.
- Prioritize responsiveness over perfect image preservation.
- Keep frames on the GPU where practical.

## Rust host guidance
- The Windows host is **Rust-first**.
- Prefer safe Rust for transport, auth, session, telemetry, and stream orchestration.
- Minimize `unsafe`.
- Isolate platform-specific or low-level interop behind narrow modules or crates.
- Do not default to C++ for the host architecture.
- If a narrow FFI boundary becomes necessary for capture or codec interop, keep that boundary small and well documented.

## AVP client guidance
- Treat the client as a **virtual display client**.
- Do not add immersive PCVR assumptions.
- Keep local overlays native.
- Keep transport/session code separate from visionOS UI/presentation code.
- Prefer hardware decode and low-latency presentation paths.

## Auth model
Use this exact mental model:
- **Apple ID token**: prove the user during login/stream creation
- **optional App Attest**: extra trust signal for the genuine AVP app/device
- **QUIC session**: the authorized active stream context
- **resume token**: only for resuming one specific stream briefly after interruption

Do not expand this into a generic OAuth platform.

## Things to avoid
- broad host session tokens
- auth on every media packet
- browser/WebRTC support in v1
- GameStream compatibility work
- PCVR features
- premature foveated streaming work before the baseline stack is stable
- overcomplicated per-device trust in v1
- pausing after every phase just to ask whether to proceed
