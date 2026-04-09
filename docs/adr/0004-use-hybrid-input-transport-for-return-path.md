# ADR 0004 – Use Hybrid Input Transport for the Return Path

**Status:** Accepted  
**Date:** 2026-04-09  
**Deciders:** HoloBridge architecture team

---

## Context

Milestone 7 adds the return input path from the Apple Vision Pro client back to the Windows host. The v1 product requirements are:

1. Cursor motion must feel immediate.
2. Clicks, scroll, keyboard state, and focus transitions must be correct and not silently disappear.
3. The streamed desktop now lives in its own visionOS window, with local controls moved into a SwiftUI ornament.
4. Ornament interaction must not also click the remote Windows desktop.

The main transport question was whether all input should use the reliable QUIC control stream or whether some input should use QUIC datagrams for lower latency.

---

## Decision

HoloBridge v1 uses a **hybrid input transport**:

- **Pointer motion** is sent over QUIC datagrams using `input-pointer-datagram-v1`.
- **Pointer button, wheel, keyboard, and input-focus state** are sent over the reliable QUIC control stream.
- Reliable pointer button and wheel messages always include the current absolute `x/y` position so the host can reposition before replaying the discrete event.

The client also uses a **dedicated stream window**:

- The connect / status UI remains in a utility shell window.
- Successful connection opens a separate stream window.
- The stream window contains only the aspect-locked video surface and native cursor overlay.
- In-session controls live in a SwiftUI ornament attached to that stream window.

---

## Rationale

### Why not send all input over the reliable control stream

Reliable ordered delivery is good for correctness, but it is a bad default for dense pointer motion:

- hover and drag updates are high-frequency and naturally coalescible
- lost pointer motion is usually acceptable because a newer position supersedes it
- retransmission and head-of-line blocking would make cursor motion feel sticky during loss or brief congestion

### Why pointer motion fits datagrams

Pointer motion is the one input class where latency matters more than perfect delivery:

- it can be sampled continuously
- the latest position is more important than every intermediate position
- it maps naturally onto the same low-latency datagram channel already used for video and pointer overlay updates

### Why discrete input stays reliable

Clicks, wheel ticks, keyboard phase transitions, and focus updates are state-changing edges:

- a dropped button-up can leave the remote machine dragging forever
- a dropped modifier-up can leave Shift / Control stuck
- a dropped focus-loss message can leave the host accepting input after the user moved to a local control

These inputs must therefore remain on the reliable control stream.

### Why reliable button / wheel messages include `x/y`

If the client sends pointer motion by datagram and then a click by reliable stream, the motion datagram may be lost while the click still arrives. Including `x/y` in the reliable click / wheel message lets the host reposition first and then replay the discrete action at the intended location.

### Why the dedicated stream window matters

The pre-Milestone-7 design showed the stream inline inside the same window as the connect UI. That made it hard to adopt visionOS-native stream controls and created ambiguous interaction zones.

Separating the stream window allows:

- a cleaner virtual-display presentation
- native ornaments for session controls
- a narrow and explicit remote-input surface

### Why ornament safety is handled locally

The client does not depend on a special visionOS ornament-focus API. Instead it uses two local rules:

1. Remote-input gestures are attached only to the visible video surface.
2. Ornament interaction suppresses outbound remote input and updates host-side input focus.

This is sufficient for v1 and avoids coupling the design to undocumented focus behavior.

---

## Consequences

- The protocol now includes:
  - capability `input-pointer-datagram-v1`
  - media datagram kind `2` for absolute pointer motion
  - reliable control messages `pointer_button`, `pointer_wheel`, `keyboard_key`, and `input_focus`
- The host now owns a session-scoped input replayer that clamps coordinates, replays Win32 input, tracks pressed state, and releases stuck input on focus loss / disconnect.
- The client now owns a dedicated stream window lifecycle plus local input suppression state for ornaments and window visibility.
- Milestone 7 intentionally supports one pointer mode only: `absolute surface`.
- Software keyboard / text entry is deferred to a follow-up phase because text entry semantics are not equivalent to raw hardware key replay.

---

## Alternatives Considered

### All input on the reliable control stream

Rejected because it would maximize correctness but degrade cursor responsiveness under loss or brief congestion.

### All input on datagrams

Rejected because dropped button-up / key-up / focus messages are too dangerous for correctness and cleanup.

### Relative pointer / explicit capture as the v1 default

Rejected for Milestone 7 because `absolute surface` is the smallest end-to-end slice that matches the current desktop-streaming UX and existing absolute pointer overlay contract.
