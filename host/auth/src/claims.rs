use serde::{Deserialize, Serialize};

/// Claims from an Apple identity token (JWT).
///
/// See: https://developer.apple.com/documentation/sign_in_with_apple/sign_in_with_apple_rest_api/authenticating_users_with_sign_in_with_apple
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleIdentityClaims {
    /// Issuer — must be `https://appleid.apple.com`.
    pub iss: String,
    /// Subject — stable, unique Apple user identifier.
    pub sub: String,
    /// Audience — must match the configured bundle ID / client ID.
    pub aud: String,
    /// Expiration time (Unix timestamp).
    pub exp: u64,
    /// Issued at time (Unix timestamp).
    pub iat: u64,
    /// User email (optional, only provided on first sign-in or if requested).
    pub email: Option<String>,
    /// Whether the email has been verified by Apple.
    pub email_verified: Option<BoolOrString>,
}

/// Apple sometimes sends `email_verified` as a string `"true"` instead of a boolean.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrString {
    Bool(bool),
    Str(String),
}

impl BoolOrString {
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Str(s) => s.eq_ignore_ascii_case("true"),
        }
    }
}

pub const APPLE_ISSUER: &str = "https://appleid.apple.com";
