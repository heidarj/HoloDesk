# Milestone 2: Sign in with Apple + Host Authorization — Implementation Plan

## Overview

This plan covers the host-side auth crate (pure Rust, buildable on Mac), protocol extensions for auth messages, server integration, and the build/test strategy. The Swift client auth is deferred to when Xcode/device is available; we stub it in the protocol layer.

---

## Phase 1: Dev Environment Setup

### 1a. Install Rust on Mac

The Mac currently has no Rust toolchain. Install it:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

This gives us `rustc`, `cargo`, and `rustup` locally. The auth crate is pure Rust (no Windows APIs), so it builds and tests natively on macOS.

### 1b. Build strategy

- **Local Mac development**: Build and test `host/auth/` and `host/transport/` natively. The auth crate has zero Windows dependencies (JWT validation, HTTP client, serde). The transport crate also has zero Windows dependencies (quinn, tokio, rustls).
- **Windows host (via SSH)**: For integration testing the full server binary. The Windows host already has Rust installed per `scripts/setup-windows-dev.ps1`. Use `ssh <user>@<win-ip>` to build and run on Windows when needed.
- **No cross-compilation needed**: Both crates are pure Rust with no platform-specific code at this milestone.

---

## Phase 2: Workspace Restructuring

### Current state
`host/transport/` is a standalone crate with its own `Cargo.toml`. There is no workspace.

### Target state
Create a Cargo workspace at `host/Cargo.toml` that contains both crates:

```
host/
  Cargo.toml          # NEW — workspace root
  transport/
    Cargo.toml        # existing, becomes workspace member
    src/
    tests/
  auth/
    Cargo.toml        # NEW
    src/
    tests/
```

### Steps

1. **Create `host/Cargo.toml`** (workspace root):
```toml
[workspace]
resolver = "2"
members = [
    "transport",
    "auth",
]
```

2. **Update `host/transport/Cargo.toml`**: No changes needed to make it a workspace member — it just needs to be listed in the workspace members. However, add `holobridge-auth` as a dependency so the server can call auth logic:
```toml
[dependencies]
holobridge-auth = { path = "../auth" }
```

3. **Create `host/auth/Cargo.toml`**:
```toml
[package]
name = "holobridge-auth"
version = "0.1.0"
edition = "2021"
rust-version = "1.78"

[dependencies]
jsonwebtoken = "9"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync", "time"] }
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Rationale
- `jsonwebtoken` v9 supports RS256 validation, JWKS key sets, and custom validation.
- `reqwest` with `rustls-tls` avoids OpenSSL dependency, keeping builds simple on both Mac and Windows.
- The workspace allows `cargo build` / `cargo test` from `host/` to build everything.

---

## Phase 3: New `host/auth/` Crate

### File structure
```
host/auth/
  Cargo.toml
  src/
    lib.rs              # public API re-exports
    apple_jwt.rs        # Apple identity token validation
    jwks.rs             # JWKS fetching and caching
    user_store.rs       # Authorized user store (config-file based)
    error.rs            # AuthError enum
  tests/
    jwt_validation.rs   # Unit/integration tests
```

### 3a. `error.rs` — Auth error types

```rust
pub enum AuthError {
    JwksFetchFailed(String),
    JwksParseError(String),
    TokenDecodeFailed(String),
    InvalidIssuer { expected: String, actual: String },
    InvalidAudience { expected: String, actual: String },
    TokenExpired,
    SubjectNotAuthorized(String),
    MissingClaim(String),
}
```

This implements `std::fmt::Display`, `std::error::Error`, and follows the same error pattern as `ProtocolError` and `TlsConfigError` in the transport crate.

### 3b. `jwks.rs` — Apple JWKS fetching and caching

**Design:**
- Struct `AppleJwksProvider` holds a cached `JwkSet` plus a `tokio::time::Instant` timestamp.
- On `get_jwks()`, if cache is fresh (within configurable TTL, default 1 hour), return cached. Otherwise fetch from `https://appleid.apple.com/auth/keys`.
- Uses `reqwest::Client` (stored in the struct) to fetch.
- Parsing uses `jsonwebtoken::jwk::JwkSet` (serde-deserializable from Apple's JSON response).
- `force_refresh()` method for testing/manual refresh.

**Key type:**
```rust
pub struct AppleJwksProvider {
    client: reqwest::Client,
    jwks_url: String,           // default: https://appleid.apple.com/auth/keys
    cache: tokio::sync::RwLock<Option<CachedJwks>>,
    cache_ttl: Duration,
}

struct CachedJwks {
    jwks: JwkSet,
    fetched_at: Instant,
}
```

**Why `RwLock`**: Multiple concurrent auth attempts can read the cached JWKS simultaneously; only a refresh needs a write lock. This is appropriate for the host scenario (few concurrent connections in v1).

### 3c. `apple_jwt.rs` — Apple identity token validation

**Design:**
- Struct `AppleTokenValidator` holds an `AppleJwksProvider` and configuration (expected `aud`, expected `iss`).
- `validate(token: &str) -> Result<AppleIdentityClaims, AuthError>` is the main entry point.

**Validation steps (matching ADR 0002):**
1. Decode the JWT header to extract the `kid` (key ID).
2. Look up the matching key in the cached JWKS. If not found, force-refresh JWKS and retry once (handles Apple key rotation).
3. Validate the JWT signature using RS256 and the matched public key via `jsonwebtoken::decode()`.
4. Validate claims:
   - `iss` must equal `https://appleid.apple.com`
   - `aud` must equal the configured Apple client ID / bundle ID
   - `exp` must be in the future
   - `sub` must be present
5. Return `AppleIdentityClaims { sub, email, email_verified, ... }`.

**Key types:**
```rust
pub struct AppleTokenValidatorConfig {
    pub expected_audience: String,  // e.g., "com.example.HoloBridge"
    pub expected_issuer: String,    // default: "https://appleid.apple.com"
}

pub struct AppleIdentityClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub iat: u64,
    pub exp: u64,
}

pub struct AppleTokenValidator {
    config: AppleTokenValidatorConfig,
    jwks_provider: AppleJwksProvider,
}
```

**`jsonwebtoken` usage pattern:**
```rust
// Decode header to get kid
let header = jsonwebtoken::decode_header(token)?;
let kid = header.kid.ok_or(AuthError::MissingClaim("kid"))?;

// Find matching JWK
let jwk = jwks.find(&kid).ok_or(AuthError::TokenDecodeFailed(...))?;
let decoding_key = DecodingKey::from_jwk(jwk)?;

// Build validation
let mut validation = Validation::new(Algorithm::RS256);
validation.set_audience(&[&config.expected_audience]);
validation.set_issuer(&[&config.expected_issuer]);
validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);

// Decode and validate
let token_data = jsonwebtoken::decode::<AppleIdentityClaims>(token, &decoding_key, &validation)?;
```

### 3d. `user_store.rs` — Authorized user store

**Design:**
- v1 uses a JSON config file for the authorized user list.
- Struct `AuthorizedUserStore` loads from a file path at startup.
- `is_authorized(sub: &str) -> bool` checks if the Apple `sub` is in the store.
- `add_user(sub: &str, display_name: &str)` / `remove_user(sub: &str)` for management.

**Config file format** (`authorized_users.json`):
```json
{
  "users": [
    {
      "apple_sub": "001234.abcdef...",
      "display_name": "Heidar",
      "added_at": "2026-04-03T12:00:00Z"
    }
  ]
}
```

**Key type:**
```rust
pub struct AuthorizedUserStore {
    file_path: PathBuf,
    users: RwLock<AuthorizedUsers>,
}

struct AuthorizedUsers {
    users: Vec<AuthorizedUser>,
}

struct AuthorizedUser {
    apple_sub: String,
    display_name: String,
    added_at: String,
}
```

**Bootstrap mode**: If the user store file does not exist, the first authenticated connection's `sub` is auto-registered as the owner (with a log warning). This avoids a chicken-and-egg problem where the user cannot configure their `sub` before they have signed in at least once. This behavior can be gated behind a config flag like `allow_first_user_bootstrap: bool`.

### 3e. `lib.rs` — Public API

```rust
pub mod apple_jwt;
pub mod error;
pub mod jwks;
pub mod user_store;

pub use apple_jwt::{AppleIdentityClaims, AppleTokenValidator, AppleTokenValidatorConfig};
pub use error::AuthError;
pub use jwks::AppleJwksProvider;
pub use user_store::AuthorizedUserStore;
```

---

## Phase 4: Protocol Message Extensions

### 4a. New `ControlMessage` variants

Add two new variants to the existing `ControlMessage` enum in `host/transport/src/protocol.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    Hello { ... },          // existing
    HelloAck { ... },       // existing
    Goodbye { ... },        // existing

    // NEW — Milestone 2
    Authenticate {
        identity_token: String,   // Apple identity JWT
    },
    AuthResult {
        success: bool,
        message: String,          // e.g., "authorized" or error reason
        user_display_name: Option<String>,
    },
}
```

**Why `Authenticate` and `AuthResult`**: These map directly to the auth flow in ADR 0002 — the client sends its identity token, the host responds with success/failure. Keeping them as control messages (rather than a separate stream) reuses the existing framing and codec infrastructure.

### 4b. Update `ControlMessage` helper methods

```rust
impl ControlMessage {
    // ... existing methods ...

    pub fn authenticate(identity_token: impl Into<String>) -> Self {
        Self::Authenticate {
            identity_token: identity_token.into(),
        }
    }

    pub fn auth_result(success: bool, message: impl Into<String>, user_display_name: Option<String>) -> Self {
        Self::AuthResult {
            success,
            message: message.into(),
            user_display_name,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            // ... existing ...
            Self::Authenticate { .. } => "authenticate",
            Self::AuthResult { .. } => "auth_result",
        }
    }

    pub fn protocol_version(&self) -> Option<u32> {
        match self {
            // ... existing ...
            Self::Authenticate { .. } | Self::AuthResult { .. } => None,
        }
    }
}
```

### 4c. Update ALPN and protocol version

Bump `DEFAULT_ALPN` from `"holobridge-m1"` to `"holobridge-m2"` to signal that the protocol now includes auth messages. This prevents an M1 client from connecting to an M2 server and being confused by auth requirements.

```rust
pub const DEFAULT_ALPN: &str = "holobridge-m2";
```

**Protocol version stays at 1** — the ALPN change is sufficient for milestone discrimination. The protocol version can be bumped if the message framing changes, which it does not here.

### 4d. Update Swift client `ControlMessage`

Add corresponding types in `client-avp/Transport/ControlMessage.swift`:

```swift
public enum ControlMessageType: String, Codable, Sendable {
    case hello
    case helloAck = "hello_ack"
    case goodbye
    case authenticate       // NEW
    case authResult = "auth_result"  // NEW
}
```

Add `identityToken`, `success`, and `userDisplayName` optional fields to the `ControlMessage` struct, following the existing flat-struct pattern.

---

## Phase 5: Connection State Machine Extension

### 5a. New auth state in `ControlConnection`

The current flow is: `Hello -> HelloAck -> [session] -> Goodbye`.

The new flow is: `Hello -> HelloAck -> Authenticate -> AuthResult -> [session] -> Goodbye`.

Add auth tracking to `ControlConnection` in `host/transport/src/connection.rs`:

```rust
pub struct ControlConnection {
    // ... existing fields ...
    auth_received: bool,       // NEW: server received Authenticate
    auth_result_sent: bool,    // NEW: server sent AuthResult
    auth_result_received: bool, // NEW: client received AuthResult
    auth_success: bool,        // NEW: was auth successful?
}
```

Update `handshake_complete()`:
```rust
pub fn handshake_complete(&self) -> bool {
    match self.role {
        ConnectionRole::Client => self.hello_ack_received && self.auth_result_received && self.auth_success,
        ConnectionRole::Server => self.hello_received && self.auth_result_sent && self.auth_success,
    }
}
```

Add a new method:
```rust
pub fn hello_exchange_complete(&self) -> bool {
    match self.role {
        ConnectionRole::Client => self.hello_ack_received,
        ConnectionRole::Server => self.hello_received,
    }
}
```

### 5b. Server-side `on_receive_as_server` updates

Update the server match to handle the new `Authenticate` message:

```rust
fn on_receive_as_server(&mut self, message: ControlMessage) -> Result<Vec<ControlMessage>, ConnectionError> {
    match message {
        ControlMessage::Hello { .. } => { /* existing */ },
        ControlMessage::Authenticate { .. } => {
            if !self.hello_received {
                return Err(ConnectionError::UnexpectedMessage { ... });
            }
            if self.auth_received {
                return Err(ConnectionError::DuplicateAuth);
            }
            self.auth_received = true;
            // Return empty — auth validation happens in the server layer, not the state machine.
            // The server will call a new method to record the auth result.
            Ok(Vec::new())
        },
        ControlMessage::AuthResult { .. } => Err(ConnectionError::UnexpectedMessage { ... }),
        // ... existing ...
    }
}
```

**Important design choice**: The `ControlConnection` state machine does NOT perform the actual JWT validation. It only tracks the protocol state transitions. The server layer (`server.rs`) calls into `holobridge-auth` to validate the token, then tells the state machine the result. This keeps the state machine pure (no async, no I/O).

Add a method for the server layer to record the auth decision:
```rust
pub fn record_auth_result(&mut self, success: bool) -> ControlMessage {
    self.auth_success = success;
    self.auth_result_sent = true;
    let msg = if success {
        ControlMessage::auth_result(true, "authorized", None)
    } else {
        ControlMessage::auth_result(false, "not authorized", None)
    };
    self.transcript.sent.push(msg.clone());
    msg
}
```

### 5c. Client-side `on_receive_as_client` updates

```rust
ControlMessage::AuthResult { success, .. } => {
    if self.auth_result_received {
        return Err(ConnectionError::DuplicateAuthResult);
    }
    self.auth_result_received = true;
    self.auth_success = success;
    Ok(Vec::new())
},
```

### 5d. New `ConnectionError` variants

```rust
pub enum ConnectionError {
    // ... existing ...
    DuplicateAuth,
    DuplicateAuthResult,
    AuthBeforeHello,
}
```

---

## Phase 6: Server Integration

### 6a. Auth configuration

Add to `TransportServerConfig` in `host/transport/src/config.rs`:

```rust
pub struct TransportServerConfig {
    // ... existing fields ...
    pub apple_audience: String,          // NEW: expected aud claim (bundle ID)
    pub authorized_users_file: PathBuf,  // NEW: path to authorized_users.json
    pub allow_first_user_bootstrap: bool, // NEW: auto-register first user
}
```

With env var support:
- `HOLOBRIDGE_APPLE_AUDIENCE` (required for auth)
- `HOLOBRIDGE_AUTHORIZED_USERS_FILE` (default: `./authorized_users.json`)
- `HOLOBRIDGE_ALLOW_FIRST_USER_BOOTSTRAP` (default: `true`)

### 6b. Server control stream flow update

Modify `run_server_control_stream()` in `host/transport/src/server.rs` to insert the auth phase after Hello/HelloAck:

```
Current flow:
  1. Read Hello from client
  2. Send HelloAck
  3. [optional] server-initiated close
  4. Wait for Goodbye

New flow:
  1. Read Hello from client
  2. Send HelloAck
  3. Read Authenticate from client           ← NEW
  4. Validate token via AppleTokenValidator   ← NEW
  5. Check user_store.is_authorized(sub)      ← NEW
  6. Send AuthResult (success or failure)     ← NEW
  7. If failure: close connection with error  ← NEW
  8. [session continues]
  9. Wait for Goodbye
```

The function signature changes to accept auth dependencies:

```rust
async fn run_server_control_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    server_initiated_close: bool,
    token_validator: &AppleTokenValidator,    // NEW
    user_store: &AuthorizedUserStore,         // NEW
) -> Result<(), TransportError>
```

### 6c. Auth failure handling

On auth failure, the server:
1. Sends `AuthResult { success: false, message: "...", ... }`.
2. Sends `Goodbye { reason: "auth-failed" }`.
3. Closes the QUIC connection with error code 1 (not 0).

```rust
connection.close(quinn::VarInt::from_u32(1), b"auth-failed");
```

### 6d. TransportServer initialization

`TransportServer::new()` or `serve_once()` creates the `AppleTokenValidator` and `AuthorizedUserStore` instances:

```rust
pub async fn serve_once(&self) -> Result<(), TransportError> {
    let jwks_provider = AppleJwksProvider::new(Default::default());
    let token_validator = AppleTokenValidator::new(
        AppleTokenValidatorConfig {
            expected_audience: self.config.apple_audience.clone(),
            ..Default::default()
        },
        jwks_provider,
    );
    let user_store = AuthorizedUserStore::load(&self.config.authorized_users_file)?;

    // ... existing endpoint setup ...

    run_server_control_stream(send, recv, ..., &token_validator, &user_store).await
}
```

### 6e. New `TransportError` variant

```rust
pub enum TransportError {
    // ... existing ...
    Auth(AuthError),  // NEW
}
```

With `From<AuthError>` impl for ergonomic `?` usage.

---

## Phase 7: Testing Strategy

### 7a. Unit tests in `host/auth/`

**`tests/jwt_validation.rs`:**

1. **Valid token test**: Construct a JWT signed with a known RSA key pair, serve the public key from a mock JWKS endpoint (or inject the `JwkSet` directly), validate it passes.
   - Use `jsonwebtoken::encode()` to create a test token with valid claims.
   - Override `jwks_url` in `AppleJwksProvider` to point to a local mock or inject cached JWKS directly.

2. **Expired token test**: Same as above but with `exp` in the past. Assert `AuthError::TokenExpired`.

3. **Wrong issuer test**: Token with `iss` != `https://appleid.apple.com`. Assert `AuthError::InvalidIssuer`.

4. **Wrong audience test**: Token with `aud` != configured bundle ID. Assert `AuthError::InvalidAudience`.

5. **Invalid signature test**: Token signed with a different key than what JWKS provides. Assert decode failure.

6. **Unknown kid test**: Token with a `kid` not in JWKS. Assert failure, and verify JWKS refresh is attempted once.

7. **User store tests**: `is_authorized` returns true for known subs, false for unknown. Bootstrap mode adds first user.

### 7b. Protocol tests in `host/transport/`

**Update `tests/codec_roundtrip.rs`:**

8. **Authenticate roundtrip**: Encode/decode an `Authenticate` message preserves the identity token.

9. **AuthResult roundtrip**: Encode/decode `AuthResult` messages for both success and failure cases.

### 7c. Connection state machine tests

10. **Happy path**: Hello -> HelloAck -> Authenticate -> AuthResult(success) -> handshake_complete() == true.

11. **Auth failure path**: Hello -> HelloAck -> Authenticate -> AuthResult(failure) -> handshake_complete() == false.

12. **Authenticate before Hello**: Rejected with `ConnectionError::AuthBeforeHello`.

13. **Duplicate Authenticate**: Rejected with `ConnectionError::DuplicateAuth`.

### 7d. Smoke test binary

**`host/transport/src/bin/auth_smoke_client.rs`:**

A binary that connects to the server, performs Hello/HelloAck, then sends a crafted JWT (either a valid test token or a deliberately invalid one) and checks the AuthResult response.

For local testing without a real Apple identity token, support a `--test-mode` flag on the server that accepts tokens signed by a local test key pair instead of Apple's JWKS. This is gated behind `HOLOBRIDGE_AUTH_TEST_MODE=true`.

### 7e. Integration test approach

Since the auth crate is pure Rust with no platform dependencies, all tests run with `cargo test` on both Mac and Windows:

```bash
cd host
cargo test --workspace
```

For JWKS fetching tests that hit the real Apple endpoint, mark them `#[ignore]` so they only run explicitly:
```bash
cargo test --workspace -- --ignored
```

---

## Phase 8: Client-Side Considerations

### What to do now (stubs)

1. **Update Swift `ControlMessage`** to include `authenticate` and `auth_result` message types and their fields. This is a mechanical change to the existing flat struct.

2. **Update Swift `TransportClient` protocol** to add:
```swift
func sendAuthenticate(identityToken: String) async throws
func awaitAuthResult() async throws -> ControlMessage
```

3. **Update ALPN** in `TransportConfiguration.swift` from `"holobridge-m1"` to `"holobridge-m2"`.

### What to defer

- **Actual Sign in with Apple flow** (`ASAuthorizationAppleIDProvider`): Requires Xcode, Apple Developer account, configured App ID with Sign in with Apple capability, and a visionOS device/simulator. This is a Phase 2b deliverable.
- **`client-avp/Auth/` directory**: Create the directory structure and a README noting what will go there, but defer the implementation.
- **App Attest**: Explicitly out of scope per ADR 0002 (optional, future milestone).

---

## Implementation Sequence

Execute these steps in order. Each step should be independently buildable/testable.

| Step | Description | Build/Test Command |
|------|-------------|-------------------|
| 1 | Install Rust on Mac | `rustc --version` |
| 2 | Create `host/Cargo.toml` workspace | `cd host && cargo check` |
| 3 | Create `host/auth/` crate skeleton (lib.rs, error.rs) | `cargo check -p holobridge-auth` |
| 4 | Implement `jwks.rs` (JWKS fetching + caching) | `cargo test -p holobridge-auth` |
| 5 | Implement `apple_jwt.rs` (token validation) | `cargo test -p holobridge-auth` |
| 6 | Implement `user_store.rs` (authorized user store) | `cargo test -p holobridge-auth` |
| 7 | Add `Authenticate`/`AuthResult` to protocol.rs + bump ALPN | `cargo test -p holobridge-transport` (existing tests still pass) |
| 8 | Add new codec roundtrip tests | `cargo test -p holobridge-transport` |
| 9 | Update connection state machine | `cargo test -p holobridge-transport` |
| 10 | Wire auth into server control stream | `cargo build -p holobridge-transport --bins` |
| 11 | Add auth config (env vars, user store path) | `cargo build -p holobridge-transport --bins` |
| 12 | Create auth smoke client binary | `cargo build -p holobridge-transport --bin auth_smoke_client` |
| 13 | Update Swift ControlMessage + TransportClient stubs | Manual review (no Swift build on this Mac) |
| 14 | End-to-end test: server + auth smoke client | Run both binaries with test mode |
| 15 | Update `docs/Status.md` | Review |

---

## Risk Mitigations

| Risk | Mitigation |
|------|-----------|
| `jsonwebtoken` crate does not support Apple's JWKS format | Apple uses standard JWK format; `jsonwebtoken` v9 supports `JwkSet` deserialization. Verified in crate docs. |
| JWKS endpoint unavailable during development | Cache mechanism + test mode with local key pair. Mark live JWKS tests as `#[ignore]`. |
| No real Apple identity token for testing | Auth smoke client uses `jsonwebtoken::encode()` with a test RSA key; server test mode accepts this key. |
| Workspace restructuring breaks existing builds | Step 2 verifies `cargo check` passes before proceeding. Existing transport tests are the regression gate. |
| Apple key rotation during operation | JWKS refresh-on-miss strategy: if `kid` not found, force one refresh before failing. |
| First-user bootstrap is a security concern | Gate behind `allow_first_user_bootstrap` config flag. Log a warning. Default to `true` for dev, recommend `false` for production. |

---

## Files to Create

| Path | Purpose |
|------|---------|
| `host/Cargo.toml` | Workspace root |
| `host/auth/Cargo.toml` | Auth crate manifest |
| `host/auth/src/lib.rs` | Public API |
| `host/auth/src/error.rs` | `AuthError` enum |
| `host/auth/src/jwks.rs` | Apple JWKS provider with caching |
| `host/auth/src/apple_jwt.rs` | Apple identity token validator |
| `host/auth/src/user_store.rs` | Authorized user store |
| `host/auth/tests/jwt_validation.rs` | Auth unit/integration tests |
| `host/transport/src/bin/auth_smoke_client.rs` | Auth smoke test binary |

## Files to Modify

| Path | Change |
|------|--------|
| `host/transport/Cargo.toml` | Add `holobridge-auth` dependency |
| `host/transport/src/protocol.rs` | Add `Authenticate`, `AuthResult` variants; bump ALPN |
| `host/transport/src/connection.rs` | Add auth state tracking, update `handshake_complete()` |
| `host/transport/src/server.rs` | Wire auth into control stream flow; add `TransportError::Auth` |
| `host/transport/src/config.rs` | Add `apple_audience`, `authorized_users_file`, `allow_first_user_bootstrap` |
| `host/transport/src/lib.rs` | Re-export new types |
| `host/transport/tests/codec_roundtrip.rs` | Add Authenticate/AuthResult roundtrip tests |
| `client-avp/Transport/ControlMessage.swift` | Add `authenticate`, `authResult` types + fields |
| `client-avp/Transport/TransportClient.swift` | Add `sendAuthenticate()`, `awaitAuthResult()` stubs |
| `client-avp/Transport/TransportConfiguration.swift` | Bump ALPN to `holobridge-m2` |
| `docs/Status.md` | Update with Milestone 2 progress |
