# HoloBridge AVP Transport

Milestone 1 adds a transport-only Swift surface under `client-avp/Transport/`. It mirrors the host control contract while staying isolated from Sign in with Apple, SwiftUI, decoding, display, and session management.

## Files

- `TransportConfiguration.swift` defines host, port, ALPN, development certificate validation, and close-behavior defaults.
- `ControlMessage.swift` mirrors the host JSON control schema and 4-byte big-endian framing.
- `TransportClient.swift` defines the small client contract for connect, `hello`, `hello_ack`, and close.
- `NetworkFrameworkQuicClient.swift` provides the first-pass Apple `Network.framework` adapter for the control-stream flow.

## Contract Defaults

- ALPN: `holobridge-m1`
- Protocol version: `1`
- Control capability: `control-stream-v1`
- Default host: `127.0.0.1:4433`

## Validation Status

This source was authored to match the Milestone 1 control contract, but it was not built or run in this Windows browser session. Local Mac or visionOS validation is still required to confirm the exact `Network.framework` QUIC runtime behavior and certificate handling.