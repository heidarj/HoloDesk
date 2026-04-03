# HoloBridge v1 – Implementation Plan

Each milestone is designed to be small enough for an autonomous coding agent to execute in a single session. Milestones build on each other; complete them in order.

---

## Milestone 0 – Repo Scaffolding and Docs

**Goal:** Establish the repository structure, documentation, and agent setup so that autonomous work can proceed without ambiguity.

**Deliverables:**
- `AGENTS.md`
- `.github/copilot-instructions.md`
- `.github/agents/continue-until-blocked.agent.md`
- `.github/instructions/host.instructions.md`
- `.github/instructions/client-avp.instructions.md`
- `docs/streaming-v1.md`
- `docs/Plan.md` (this file)
- `docs/Status.md`
- `docs/adr/0001-use-http3-quic-instead-of-rtp-rtsp.md`
- `docs/adr/0002-auth-model-apple-id-token-quic-session-resume-token.md`
- `README.md`
- `host/` directory scaffold (placeholder)
- `client-avp/` directory scaffold (placeholder)

**Acceptance Criteria:**
- All required files exist and are internally consistent.
- Docs describe the same architecture and auth model.
- The custom agent is defined.
- The repo is ready for autonomous milestone work.

**Validation Steps:**
1. Verify all files listed in deliverables exist.
2. Cross-check that `docs/streaming-v1.md`, `AGENTS.md`, and both ADRs agree on transport, auth, and codec choices.
3. Verify `docs/Status.md` is populated.

---

## Milestone 1 – QUIC / HTTP3 Transport Skeleton

**Goal:** Establish a minimal QUIC/HTTP3 transport layer on both host and client that can open a connection, exchange a control message, and close cleanly.

**Deliverables:**
- `host/transport/` – QUIC server skeleton (listen, accept connection, open/receive control stream, close)
- `client-avp/Transport/` – QUIC client skeleton (connect, open/send control stream, receive response, close)
- Loopback integration test or documented manual test procedure

**Acceptance Criteria:**
- Host can accept a QUIC connection from the AVP client (or a test client).
- Client can establish a QUIC connection to the host.
- A simple control message can be sent from client → host and a response sent host → client.
- Connection can be closed cleanly from either side.

**Validation Steps:**
1. Run host transport skeleton; verify it listens and accepts connections.
2. Run client transport skeleton against host; verify control message round-trip.
3. Verify clean shutdown on both sides.

**Notes:**
- Use a well-maintained QUIC/HTTP3 library. The host uses quinn (pure Rust). On Apple platforms, use `Network.framework` with QUIC. See ADR 0003.
- Do not implement auth or video in this milestone. Stub those interfaces.
- Document the chosen QUIC library and version in a new ADR if the choice requires justification.

---

## Milestone 2 – Sign in with Apple + Host Authorization

**Goal:** AVP client signs in with Apple and the host validates the identity token to authorize the user.

**Deliverables:**
- `client-avp/Auth/` – Sign in with Apple flow; sends identity token to host
- `host/auth/` – Apple identity token validation; Apple `sub` → local user mapping
- A minimal authorized user store (in-memory or config file for v1)

**Acceptance Criteria:**
- Client can complete Sign in with Apple and obtain an identity token.
- Client sends the identity token to the host over the QUIC control stream.
- Host fetches Apple JWKS and validates the token (signature, `iss`, `aud`, `exp`).
- Host maps the `sub` claim to an authorized user or returns an auth failure.
- A rejected token causes the host to close the QUIC connection with an appropriate error.

**Validation Steps:**
1. Sign in with Apple on device/simulator; confirm identity token is obtained.
2. Send identity token to host; confirm host validates successfully.
3. Send an expired or tampered token; confirm host rejects and closes connection.
4. Verify JWKS fetch and caching behavior.

**Notes:**
- The `aud` claim must match the host's configured Apple client ID / bundle ID.
- Cache JWKS with appropriate TTL; do not fetch on every request.
- Do not use Apple access tokens as host API tokens (ADR 0002).

---

## Milestone 3 – Stream Lifecycle + Resume Token

**Goal:** The host can create and terminate a stream session, and issue a resume token on interruption.

**Deliverables:**
- `host/session/` – session creation, hold, resume, termination
- `host/auth/` – resume token issue, validate, invalidate
- `client-avp/Session/` – session lifecycle management, resume token storage and use

**Acceptance Criteria:**
- Host creates a session after successful auth.
- Host holds session state on QUIC interruption and issues a resume token.
- Client can reconnect using the resume token within the token's TTL.
- Host validates the resume token, resumes the session, and invalidates the token.
- Expired resume tokens are rejected.
- Session terminates cleanly and releases resources.

**Validation Steps:**
1. Establish authenticated session; verify session state on host.
2. Simulate QUIC interruption; confirm resume token is issued.
3. Reconnect with valid resume token within TTL; confirm session resumes.
4. Reconnect with expired resume token; confirm rejection.
5. Terminate session; confirm resource cleanup on host.

---

## Milestone 4 – Display Enumeration + DXGI Capture

**Goal:** The host can enumerate displays and capture frames from the primary display using DXGI Desktop Duplication.

**Deliverables:**
- `host/capture/` – DXGI backend: enumerate displays, open duplication, acquire frames
- `ICaptureBackend` interface for future alternative backends
- Captured frames available as `ID3D11Texture2D` on the GPU

**Acceptance Criteria:**
- Host enumerates available displays.
- Host opens `IDXGIOutputDuplication` for the target display.
- Host acquires frames at the target frame rate (e.g., 60 fps).
- Frames are available as GPU textures (`ID3D11Texture2D`).
- No CPU round-trip in the capture-to-encoder path.

**Validation Steps:**
1. Run capture module; confirm display enumeration output.
2. Acquire frames at target rate; confirm no dropped frames in steady state.
3. Verify frames remain on GPU (no readback to CPU in hot path).
4. Confirm `ICaptureBackend` interface is defined and DXGI implementation conforms.

---

## Milestone 5 – H.264 Encode Path

**Goal:** Host encodes captured DXGI frames as H.264 NALUs using a hardware encoder.

**Deliverables:**
- `host/encode/` – H.264 encoder pipeline (MFT or hardware vendor SDK)
- `IVideoEncoder` interface
- Encoded NALUs available for transport layer consumption

**Acceptance Criteria:**
- Host encodes captured frames as H.264 NALUs.
- Encoder uses low-latency preset (no B-frames, minimal buffering).
- Periodic IDR refresh (e.g., every 2 seconds).
- Encoded NALUs are produced at the capture frame rate.
- Zero-copy path from DXGI texture to encoder input surface (where hardware supports it).

**Validation Steps:**
1. Run capture + encode pipeline; verify NALUs are produced.
2. Write NALUs to a file; verify with `ffprobe` or equivalent that H.264 stream is valid.
3. Verify IDR frames appear at the configured refresh interval.
4. Measure encode latency; confirm it is within target (< 10 ms per frame as a starting target).

---

## Milestone 6 – Video Transport + AVP Decode / Display

**Goal:** Host streams H.264 NALUs over QUIC datagrams to the AVP client, which decodes and displays them.

**Deliverables:**
- `host/transport/` – NALU framing and QUIC datagram send
- `client-avp/Transport/` – QUIC datagram receive and NALU reassembly
- `client-avp/Decode/` – VideoToolbox H.264 decoder
- `client-avp/Display/` – Metal / SwiftUI display of decoded frames

**Acceptance Criteria:**
- Host sends encoded H.264 NALUs over QUIC datagrams.
- Client receives datagrams and reassembles NALUs.
- Client decodes H.264 using VideoToolbox.
- Client displays decoded frames as a flat virtual display.
- Video is visible and reasonably smooth at the target frame rate.
- Packet loss results in brief artifact or frame skip, not a stream stall.

**Validation Steps:**
1. Run end-to-end pipeline; verify video is visible on AVP.
2. Simulate packet loss (e.g., traffic shaping); verify recovery within IDR interval.
3. Measure end-to-end latency (capture → display); target < 50 ms on local network.
4. Verify decoded frames remain on GPU (no CPU readback in display path).

---

## Milestone 7 – Input Return Path

**Goal:** User input from AVP (pointer, keyboard, scroll) is forwarded to the host and replayed.

**Deliverables:**
- `client-avp/Input/` – visionOS input capture; encode and send over QUIC control stream
- `host/input/` – receive input events; replay via `SendInput` or equivalent Win32 API

**Acceptance Criteria:**
- Pointer movement on AVP moves the cursor on the Windows desktop.
- Click events on AVP trigger click events on Windows.
- Scroll events on AVP trigger scroll on Windows.
- Keyboard input (if applicable) is forwarded correctly.
- Input latency is low (< 50 ms on local network as a starting target).

**Validation Steps:**
1. Move pointer on AVP; confirm cursor moves on host desktop.
2. Click on a UI element on AVP; confirm the click registers on Windows.
3. Test scroll; confirm scroll registers on Windows.
4. Measure input round-trip latency.

---

## Milestone 8 – Audio Streaming

**Goal:** Host captures system audio and streams it to AVP client.

**Deliverables:**
- `host/audio/` – WASAPI audio capture
- Audio encode (Opus or AAC)
- Audio transport over QUIC (separate stream or datagram)
- `client-avp/Audio/` – decode and play audio

**Acceptance Criteria:**
- System audio from host is captured and streamed.
- Client receives, decodes, and plays audio in sync with video (approximate).
- Audio is not present in the client if no audio is playing on host.

**Validation Steps:**
1. Play audio on host; confirm audio plays on AVP.
2. Stop audio on host; confirm silence on AVP.
3. Verify audio/video are approximately in sync.

---

## Milestone 9 – Reconnect / Recovery / Telemetry Polish

**Goal:** Harden the reconnect flow, improve recovery from packet loss, and add basic telemetry.

**Deliverables:**
- Robust reconnect flow using resume token (from Milestone 3)
- IDR-based recovery tuning
- Per-session telemetry logging (capture latency, encode latency, transport RTT, decode latency, frame drop rate)

**Acceptance Criteria:**
- Network interruption of up to 30 seconds reconnects automatically using resume token.
- Packet loss of up to 5% does not cause stream stall (IDR recovery).
- Telemetry data is logged per session.
- Resume token expiry and session termination are handled gracefully.

**Validation Steps:**
1. Simulate 10-second network interruption; confirm automatic reconnect.
2. Simulate 5% packet loss; confirm stream recovers within IDR interval.
3. Verify telemetry log output after a session.

---

## Milestone 10 – Optional Enhancements

These are optional and should only be started after Milestone 9 is complete and validated.

**10a – Windows.Graphics.Capture (WGC) Backend**
- Implement `ICaptureBackend` using WGC.
- Add selection logic: prefer DXGI; fall back to WGC for window-level capture.
- Acceptance: WGC backend produces valid frames; DXGI remains default.

**10b – HEVC Codec**
- Implement `IVideoEncoder` using H.265/HEVC hardware encoder.
- Add codec negotiation in session setup.
- Acceptance: HEVC stream is decodable on AVP; H.264 remains default.

**10c – App Attest Enforcement**
- Optionally require Apple App Attest assertion from the client as an additional trust signal.
- Acceptance: Host can validate App Attest assertion; clients without attest still work in non-enforced mode.
