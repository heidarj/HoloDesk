use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Expected `aud` claim — the Apple bundle ID / client ID.
    pub apple_bundle_id: String,
    /// JWKS cache TTL in seconds (default: 3600).
    pub jwks_cache_ttl_secs: u64,
    /// Path to the authorized user store JSON file.
    pub user_store_path: PathBuf,
    /// When true, auto-register the first authenticated user.
    pub bootstrap_mode: bool,
    /// When true, accept tokens signed by a local test key instead of Apple JWKS.
    pub test_mode: bool,
    /// Path to the PEM-encoded RSA public key for test mode.
    pub test_public_key_pem: Option<PathBuf>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        Self {
            apple_bundle_id: std::env::var("HOLOBRIDGE_AUTH_BUNDLE_ID")
                .unwrap_or_else(|_| "cloud.hr5.HoloBridge".to_owned()),
            jwks_cache_ttl_secs: std::env::var("HOLOBRIDGE_AUTH_JWKS_TTL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            user_store_path: std::env::var("HOLOBRIDGE_AUTH_USER_STORE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("authorized_users.json")),
            bootstrap_mode: std::env::var("HOLOBRIDGE_AUTH_BOOTSTRAP")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(true),
            test_mode: std::env::var("HOLOBRIDGE_AUTH_TEST_MODE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            test_public_key_pem: std::env::var("HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY")
                .ok()
                .map(PathBuf::from),
        }
    }
}
