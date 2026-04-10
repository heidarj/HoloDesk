# Resolution Change Stall — Investigation Log

Tracking the investigation of a recurring bug where the host video stream
stalls and dies after a desktop resolution change.

---

## Attempt 1 — DXGI re-duplication after bounds change

**Date:** 2026-04-10

### Symptom (log: `e2e-host-20260410-151755.log`)

Desktop resolution changed 3840x2160 -> 2560x1600 during an active stream.
DXGI access-lost recovery succeeded, encoder was recreated for the new
dimensions, but then the video worker stalled:

- `current_stage = "waiting_for_frame"`
- `consecutive_wait_timeouts = 0` (i.e. `AcquireNextFrame` was not returning
  at all — not even timing out)
- `since_last_frame_ms` grew to 8121 ms before the watchdog killed the stream

### Hypothesis

After DXGI access-lost recovery with changed bounds, the new
`IDXGIOutputDuplication` handle can be in a transitional compositor state
where `AcquireNextFrame` blocks indefinitely.  The 100 ms sleep before
`DuplicateOutput` in `recover_from_duplication_loss` is not enough.

### Fix applied

`host/capture/src/windows_backend.rs` — `recover_from_duplication_loss()`:
Added a second `DuplicateOutput` call with a 200 ms settling delay after
detecting changed bounds.  The idea was to get a clean duplication handle
after the compositor finishes transitioning.

### Result: DID NOT FIX

Re-tested (log: `e2e-host-20260410-160452.log`).  The re-duplication failed
with `E_INVALIDARG (0x80070057)` — the output likely rejects a second
concurrent duplication.  First handle was kept.

More importantly, the stall this time was in a **completely different stage**:

- `current_stage = "encoding_frame"` (not `waiting_for_frame`)
- `stage_elapsed_ms = 3196` when stall detected (`blocking-stage-timeout`)
- `frames_sent` went from 211 to 231 after encoder recreation — capture was
  **working fine**, the MFT H.264 encoder was what hung

### Revised analysis

The first log's `waiting_for_frame` stall and the second log's
`encoding_frame` stall are likely the same underlying GPU-level problem
manifesting in whichever component happens to be blocking on the GPU when the
device becomes unresponsive.

The root cause is in how the encoder is replaced during a resolution change:

1. `server.rs:809` — A new `MfH264Encoder` is created on the **same D3D
   device** as the old encoder
2. `server.rs:811` — `encoder = new_encoder` drops the old encoder
3. The old encoder's `Drop` only sends `MFT_MESSAGE_NOTIFY_END_STREAMING` —
   **no `COMMAND_FLUSH`**, so pending GPU operations are not discarded
4. Both old and new MFTs briefly coexist on the same D3D device; the old MFT
   may still have in-flight GPU work when the new MFT starts encoding
5. After ~20 frames the new MFT's internal pipeline hits a GPU resource
   conflict or stale state and hangs in `ProcessInput` / `ProcessOutput`

Compare the `Drop` implementation:
```rust
// Drop — incomplete
MFT_MESSAGE_NOTIFY_END_STREAMING
ShutdownObject

// abort_handle — correct
MFT_MESSAGE_COMMAND_FLUSH          // discard pending I/O
MFT_MESSAGE_NOTIFY_END_OF_STREAM   // signal no more input
MFT_MESSAGE_NOTIFY_END_STREAMING   // transition to idle
```

---

## Attempt 2 — Flush old encoder before creating replacement

**Date:** 2026-04-10

### Fix applied

Two changes:

1. **`host/transport/src/server.rs`** — encoder rebuild path: Before creating
   the new encoder, explicitly abort the old encoder via its abort handle.
   This sends `COMMAND_FLUSH` + `END_OF_STREAM` + `END_STREAMING`, ensuring
   all pending GPU operations are discarded and the MFT reaches idle state
   before the new encoder is created on the same D3D device.

2. **`host/encode/src/windows_backend.rs`** — `Drop for MfH264Encoder`:
   Updated to match the abort handle sequence (`COMMAND_FLUSH` +
   `END_OF_STREAM` + `END_STREAMING` + `ShutdownObject`).  This ensures
   proper cleanup in all drop paths, not just the rebuild path.

Also reverted the re-duplication fix from Attempt 1, since the stall was in
the encoder, not capture.  The existing single `DuplicateOutput` + 100 ms
settle is sufficient.

### Result

(pending manual e2e verification)
