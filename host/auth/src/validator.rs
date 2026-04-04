use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use tracing::{debug, info};

use crate::{
    claims::{AppleIdentityClaims, APPLE_ISSUER},
    config::AuthConfig,
    error::AuthError,
    jwks::AppleJwksProvider,
};

pub struct TokenValidator {
    jwks_provider: Option<AppleJwksProvider>,
    test_decoding_key: Option<DecodingKey>,
    expected_audience: String,
    test_mode: bool,
}

impl TokenValidator {
    /// Create a new validator. In test mode, uses a local RSA key; otherwise fetches Apple JWKS.
    pub async fn new(config: &AuthConfig) -> Result<Self, AuthError> {
        if config.test_mode {
            let test_key = if let Some(ref pem_path) = config.test_public_key_pem {
                let pem = tokio::fs::read(pem_path)
                    .await
                    .map_err(|e| AuthError::Internal(format!("reading test public key: {e}")))?;
                DecodingKey::from_rsa_pem(&pem)
                    .map_err(|e| AuthError::Internal(format!("parsing test public key PEM: {e}")))?
            } else {
                return Err(AuthError::Internal(
                    "test mode enabled but no test public key path configured \
                     (set HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY)"
                        .to_owned(),
                ));
            };

            info!("auth validator initialized in TEST MODE");
            Ok(Self {
                jwks_provider: None,
                test_decoding_key: Some(test_key),
                expected_audience: config.apple_bundle_id.clone(),
                test_mode: true,
            })
        } else {
            info!("auth validator initialized with Apple JWKS");
            Ok(Self {
                jwks_provider: Some(AppleJwksProvider::new(config.jwks_cache_ttl_secs)),
                test_decoding_key: None,
                expected_audience: config.apple_bundle_id.clone(),
                test_mode: false,
            })
        }
    }

    /// Validate an identity token (JWT) and return the claims.
    pub async fn validate(&self, token: &str) -> Result<AppleIdentityClaims, AuthError> {
        let header = decode_header(token)
            .map_err(|e| AuthError::TokenInvalid(format!("decoding header: {e}")))?;

        debug!(alg = ?header.alg, kid = ?header.kid, "decoded JWT header");

        let decoding_key = if self.test_mode {
            self.test_decoding_key
                .as_ref()
                .ok_or_else(|| AuthError::Internal("test decoding key not set".to_owned()))?
                .clone()
        } else {
            let kid = header
                .kid
                .as_deref()
                .ok_or_else(|| AuthError::TokenInvalid("missing kid in header".to_owned()))?;

            self.jwks_provider
                .as_ref()
                .expect("JWKS provider must be set in production mode")
                .get_decoding_key(kid)
                .await?
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.expected_audience]);
        validation.set_issuer(&[APPLE_ISSUER]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp", "iat"]);

        // In test mode, also allow tokens from a test issuer.
        if self.test_mode {
            validation.set_issuer(&[APPLE_ISSUER, "https://test.holobridge.local"]);
        }

        let token_data = decode::<AppleIdentityClaims>(token, &decoding_key, &validation).map_err(
            |e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => AuthError::InvalidAudience {
                    expected: self.expected_audience.clone(),
                    actual: "token audience mismatch".to_owned(),
                },
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                    AuthError::InvalidIssuer("token issuer mismatch".to_owned())
                }
                _ => AuthError::TokenInvalid(e.to_string()),
            },
        )?;

        info!(sub = %token_data.claims.sub, "token validated successfully");
        Ok(token_data.claims)
    }
}
