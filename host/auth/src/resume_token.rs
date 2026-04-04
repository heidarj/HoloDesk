use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{AuthConfig, AuthError};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeTokenClaims {
    pub session_id: String,
    pub expires_at_unix_secs: u64,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedResumeToken {
    pub token: String,
    pub claims: ResumeTokenClaims,
    pub ttl_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ResumeTokenService {
    secret: Vec<u8>,
    ttl: Duration,
}

impl ResumeTokenService {
    pub fn new(config: &AuthConfig) -> Result<Self, AuthError> {
        let secret = match &config.resume_token_secret {
            Some(secret) if !secret.trim().is_empty() => secret.as_bytes().to_vec(),
            _ => random_bytes(32),
        };

        Self::from_secret(secret, config.resume_token_ttl_secs)
    }

    pub fn from_secret(secret: Vec<u8>, ttl_secs: u64) -> Result<Self, AuthError> {
        if secret.is_empty() {
            return Err(AuthError::Internal(
                "resume token secret must not be empty".to_owned(),
            ));
        }
        if ttl_secs == 0 {
            return Err(AuthError::Internal(
                "resume token ttl must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            secret,
            ttl: Duration::from_secs(ttl_secs),
        })
    }

    pub fn ttl_secs(&self) -> u64 {
        self.ttl.as_secs()
    }

    pub fn issue(&self, session_id: &str) -> Result<IssuedResumeToken, AuthError> {
        let claims = ResumeTokenClaims {
            session_id: session_id.to_owned(),
            expires_at_unix_secs: now_unix_secs() + self.ttl.as_secs(),
            nonce: URL_SAFE_NO_PAD.encode(random_bytes(16)),
        };
        let token = self.sign_claims(&claims)?;
        Ok(IssuedResumeToken {
            token,
            claims,
            ttl_secs: self.ttl_secs(),
        })
    }

    pub fn validate(&self, token: &str) -> Result<ResumeTokenClaims, AuthError> {
        let (payload_b64, signature_b64) = token.split_once('.').ok_or_else(|| {
            AuthError::ResumeTokenInvalid("missing signature separator".to_owned())
        })?;

        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|e| AuthError::ResumeTokenInvalid(format!("decoding payload: {e}")))?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_b64)
            .map_err(|e| AuthError::ResumeTokenInvalid(format!("decoding signature: {e}")))?;

        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|e| AuthError::Internal(format!("initializing HMAC: {e}")))?;
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| AuthError::ResumeTokenInvalid("signature mismatch".to_owned()))?;

        let claims: ResumeTokenClaims = serde_json::from_slice(&payload)
            .map_err(|e| AuthError::ResumeTokenInvalid(format!("parsing payload: {e}")))?;

        if claims.expires_at_unix_secs <= now_unix_secs() {
            return Err(AuthError::ResumeTokenExpired);
        }

        Ok(claims)
    }

    fn sign_claims(&self, claims: &ResumeTokenClaims) -> Result<String, AuthError> {
        let payload = serde_json::to_vec(claims)
            .map_err(|e| AuthError::Internal(format!("serializing resume token claims: {e}")))?;
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|e| AuthError::Internal(format!("initializing HMAC: {e}")))?;
        mac.update(&payload);
        let signature = mac.finalize().into_bytes();
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}
