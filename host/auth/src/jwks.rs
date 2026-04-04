use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::AuthError;

const APPLE_JWKS_URL: &str = "https://appleid.apple.com/auth/keys";

pub struct AppleJwksProvider {
    cache: RwLock<Option<CachedJwks>>,
    ttl: Duration,
}

struct CachedJwks {
    jwk_set: JwkSet,
    fetched_at: Instant,
}

impl AppleJwksProvider {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(None),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Get the decoding key for a given key ID. Fetches JWKS if not cached or expired.
    /// If the kid is not found, forces one refresh before giving up.
    pub async fn get_decoding_key(
        &self,
        kid: &str,
    ) -> Result<jsonwebtoken::DecodingKey, AuthError> {
        // Try cache first.
        if let Some(key) = self.lookup_cached(kid).await {
            return Ok(key);
        }

        // Cache miss or expired — fetch.
        self.refresh().await?;

        if let Some(key) = self.lookup_cached(kid).await {
            return Ok(key);
        }

        Err(AuthError::KeyNotFound(kid.to_owned()))
    }

    async fn lookup_cached(&self, kid: &str) -> Option<jsonwebtoken::DecodingKey> {
        let cache = self.cache.read().await;
        let cached = cache.as_ref()?;

        if cached.fetched_at.elapsed() > self.ttl {
            debug!("JWKS cache expired");
            return None;
        }

        find_key_in_set(&cached.jwk_set, kid)
    }

    async fn refresh(&self) -> Result<(), AuthError> {
        info!("fetching Apple JWKS from {APPLE_JWKS_URL}");
        let response = reqwest::get(APPLE_JWKS_URL)
            .await
            .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

        let jwk_set: JwkSet = response
            .json()
            .await
            .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

        info!(keys = jwk_set.keys.len(), "fetched Apple JWKS");

        let mut cache = self.cache.write().await;
        *cache = Some(CachedJwks {
            jwk_set,
            fetched_at: Instant::now(),
        });

        Ok(())
    }
}

fn find_key_in_set(jwk_set: &JwkSet, kid: &str) -> Option<jsonwebtoken::DecodingKey> {
    for jwk in &jwk_set.keys {
        if jwk.common.key_id.as_deref() == Some(kid) {
            match jsonwebtoken::DecodingKey::from_jwk(jwk) {
                Ok(key) => {
                    debug!(kid, "found matching JWKS key");
                    return Some(key);
                }
                Err(e) => {
                    warn!(kid, error = %e, "failed to convert JWK to decoding key");
                }
            }
        }
    }
    None
}
