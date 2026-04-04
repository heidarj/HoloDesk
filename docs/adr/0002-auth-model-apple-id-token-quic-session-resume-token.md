# ADR 0002 – Auth Model: Apple ID Token + QUIC Session + Stream Resume Token

**Status:** Accepted  
**Date:** 2026-03  
**Deciders:** HoloBridge architecture team

---

## Context

HoloBridge v1 needs an authentication and authorization model for:
1. Proving the identity of the AVP client user to the host.
2. Authorizing an active streaming session.
3. Allowing the client to resume a stream after a brief network interruption without re-authenticating.

The platform constraints are:
- The client is a native Apple Vision Pro application.
- The host is a Windows application, not a web server.
- There is no third-party identity provider infrastructure to maintain.
- The product must not require users to create a separate HoloBridge account in v1.

---

## Decision

HoloBridge v1 uses a three-part auth model:

1. **Sign in with Apple** – The AVP client uses `ASAuthorizationAppleIDProvider` to sign in. The resulting Apple identity token (a short-lived JWT) is sent to the host at session creation.
2. **QUIC session as the active auth context** – After the host validates the identity token, the QUIC connection itself is the authorized session context. No per-packet tokens or headers.
3. **Stream-specific resume token** – If the QUIC session is interrupted, the host issues a short-lived (e.g., 60-minute), stream-scoped resume token. The client uses this token only to resume that one stream.

---

## Rationale

### Why Sign in with Apple

- The client is a native AVP app, so `ASAuthorizationAppleIDProvider` is the natural Apple-platform auth mechanism.
- Sign in with Apple provides a cryptographically signed identity token (JWT) with a stable `sub` (subject) claim that uniquely identifies the user across sessions.
- Apple manages credential security, 2FA, and account recovery, so the host does not need to implement user account infrastructure.
- Sign in with Apple is required for apps distributed on the App Store that offer third-party sign-in, and is the natural choice for a visionOS native app.
- Users already trust Apple with their identity; no new account is required.

### Why the Apple identity token is used only at session creation

- The identity token is short-lived (typically 10 minutes). It is not appropriate for long-lived session auth.
- Re-validating the identity token on every request would require repeated Apple JWKS fetches and add latency.
- Once the user is authenticated at session creation, the streaming session is the trust context. The QUIC connection provides mutual authentication and encryption; there is no need to re-establish identity per packet.

### Why the QUIC session is the active auth context

- QUIC provides a mutually authenticated, encrypted connection. A valid QUIC session from an authenticated client is itself a trust signal.
- Per-packet auth tokens (e.g., a Bearer token on every media datagram) add overhead, latency, and complexity with no meaningful security benefit once the session is established.
- Session-level auth is standard practice in streaming protocols (e.g., TLS sessions, SSH sessions).

### Why not use Apple access tokens as host API tokens

- Apple access tokens are short-lived and intended for querying Apple APIs (e.g., the Apple ID server), not for authenticating to third-party services.
- Using Apple access tokens as host API tokens would require the host to call Apple's token introspection endpoint on every request, adding external dependency and latency.
- The Apple identity token (JWT) contains all the claims the host needs (`sub`, `iss`, `aud`, `exp`) and can be validated offline after the initial JWKS fetch.

### Why not mint broad long-lived host auth tokens in v1

- Broad long-lived tokens are a security liability: if leaked, they grant extended access.
- A full token management system (issuance, refresh, revocation) adds significant complexity.
- In v1, the QUIC session provides the session auth context. A resume token handles the specific reconnection use case without needing a general-purpose token system.

### Why a short-lived stream-specific resume token

- Network interruptions (Wi-Fi handoff, brief outages) are common. Requiring full re-authentication (Sign in with Apple) after a brief interruption would be a poor user experience.
- A resume token allows the client to reconnect quickly without re-presenting the Apple identity token.
- Scoping the token to a single stream prevents it from being used to start new streams or access other resources.
- Short lifetime (e.g., 60 minutes) limits the window of exposure if the token is leaked.
- Invalidating the token after first successful use prevents replay attacks.

### Optional: App Attest

Apple's App Attest API can verify that the client is a genuine instance of the expected AVP app running on a real Apple device. This is an additional trust signal, not a replacement for identity token validation. It is optional in v1 because:
- It adds complexity to the attestation validation flow on the host.
- The primary security guarantee comes from the Apple identity token.
- App Attest can be made mandatory in a future milestone once the core auth flow is solid.

---

## Consequences

- The host must implement Apple JWKS fetching and JWT validation (signature, `iss`, `aud`, `exp`, `sub`).
- The host must maintain a mapping from Apple `sub` to a locally authorized user record (in-memory or config-based in v1).
- The host must implement resume token issuance (e.g., HMAC-SHA256 over session ID + expiry timestamp), validation, and invalidation.
- The Apple client app must be configured with a valid Apple Services bundle ID and Sign in with Apple entitlement. These values must be provided by the developer and are not invented.
- The host must be configured with the expected `aud` (Apple client ID / bundle ID) for JWT validation.

---

## Alternatives Considered

### Generic OAuth / OpenID Connect server

A self-hosted OAuth authorization server would allow more flexible token management. Rejected because:
- Adds significant infrastructure complexity (auth server, token storage, refresh flow).
- Not necessary when Sign in with Apple provides all the identity claims needed.
- Non-goal: HoloBridge is not a generic OAuth platform.

### Moonlight/GameStream PIN pairing

Moonlight's pairing model uses a PIN-based device pairing flow. Rejected because:
- Moonlight/GameStream compatibility is explicitly a non-goal for v1.
- PIN-based pairing does not use a recognized identity provider.
- Sign in with Apple provides a more secure and user-friendly identity model.

### No authentication (local network only)

Accepting all connections on the local network without authentication. Rejected because:
- Provides no user identity, making per-user access control impossible.
- Assumes a trusted network, which may not hold even on a home network.
- Sign in with Apple is straightforward to implement on visionOS and provides real security value.
