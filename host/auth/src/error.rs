use std::{error::Error, fmt};

#[derive(Debug)]
pub enum AuthError {
    /// JWT decoding or signature verification failed.
    TokenInvalid(String),
    /// Token has expired.
    TokenExpired,
    /// The `iss` claim does not match the expected Apple issuer.
    InvalidIssuer(String),
    /// The `aud` claim does not match the configured bundle ID.
    InvalidAudience { expected: String, actual: String },
    /// The `sub` claim is not in the authorized user store.
    UserNotAuthorized(String),
    /// Failed to fetch or parse Apple JWKS.
    JwksFetchError(String),
    /// No matching key ID found in JWKS.
    KeyNotFound(String),
    /// Internal error (I/O, serialization, etc.).
    Internal(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenInvalid(msg) => write!(f, "invalid token: {msg}"),
            Self::TokenExpired => write!(f, "token has expired"),
            Self::InvalidIssuer(iss) => write!(f, "invalid issuer: {iss}"),
            Self::InvalidAudience { expected, actual } => {
                write!(f, "invalid audience: expected {expected}, got {actual}")
            }
            Self::UserNotAuthorized(sub) => write!(f, "user not authorized: {sub}"),
            Self::JwksFetchError(msg) => write!(f, "JWKS fetch error: {msg}"),
            Self::KeyNotFound(kid) => write!(f, "no matching key for kid: {kid}"),
            Self::Internal(msg) => write!(f, "internal auth error: {msg}"),
        }
    }
}

impl Error for AuthError {}
