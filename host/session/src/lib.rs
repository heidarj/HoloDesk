use std::{
    collections::HashMap,
    error::Error,
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use holobridge_auth::{IssuedResumeToken, ResumeTokenClaims, ResumeTokenService};
use rand::RngCore;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Held,
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: String,
    pub user_sub: String,
    pub user_display_name: Option<String>,
    pub state: SessionState,
    pub created_at_unix_secs: u64,
    pub hold_expires_at_unix_secs: Option<u64>,
    pub current_resume_nonce: Option<String>,
    pub reconnect_count: u32,
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedSession {
    pub session_id: String,
    pub user_display_name: Option<String>,
    pub resume_token: String,
    pub resume_token_ttl_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedSession {
    pub session_id: String,
    pub user_display_name: Option<String>,
    pub resume_token: String,
    pub resume_token_ttl_secs: u64,
    pub reconnect_count: u32,
}

#[derive(Debug)]
pub enum SessionError {
    SessionNotFound(String),
    SessionNotResumable(String),
    ResumeTokenMismatch(String),
    SessionExpired(String),
    Internal(String),
}

pub struct SessionManager {
    sessions: RwLock<HashMap<String, SessionRecord>>,
    resume_tokens: ResumeTokenService,
    hold_ttl_secs: u64,
}

impl SessionManager {
    pub fn new(
        resume_tokens: ResumeTokenService,
        hold_ttl_secs: u64,
    ) -> Result<Self, SessionError> {
        if hold_ttl_secs == 0 {
            return Err(SessionError::Internal(
                "hold ttl must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            sessions: RwLock::new(HashMap::new()),
            resume_tokens,
            hold_ttl_secs,
        })
    }

    pub fn resume_token_ttl_secs(&self) -> u64 {
        self.resume_tokens.ttl_secs()
    }

    pub fn hold_ttl_secs(&self) -> u64 {
        self.hold_ttl_secs
    }

    pub async fn create_session(
        &self,
        user_sub: &str,
        user_display_name: Option<String>,
    ) -> Result<CreatedSession, SessionError> {
        self.prune_expired().await;

        let session_id = generate_session_id();
        let issued = self
            .resume_tokens
            .issue(&session_id)
            .map_err(|e| SessionError::Internal(e.to_string()))?;
        let record = SessionRecord {
            session_id: session_id.clone(),
            user_sub: user_sub.to_owned(),
            user_display_name: user_display_name.clone(),
            state: SessionState::Active,
            created_at_unix_secs: now_unix_secs(),
            hold_expires_at_unix_secs: None,
            current_resume_nonce: Some(issued.claims.nonce.clone()),
            reconnect_count: 0,
            termination_reason: None,
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), record);

        Ok(CreatedSession {
            session_id,
            user_display_name,
            resume_token: issued.token,
            resume_token_ttl_secs: issued.ttl_secs,
        })
    }

    pub async fn hold_session(&self, session_id: &str) -> Result<(), SessionError> {
        self.prune_expired().await;

        let mut sessions = self.sessions.write().await;
        let record = sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionError::SessionNotFound(session_id.to_owned()))?;

        if record.state == SessionState::Terminated {
            return Ok(());
        }

        record.state = SessionState::Held;
        record.hold_expires_at_unix_secs = Some(now_unix_secs() + self.hold_ttl_secs);
        record.termination_reason = None;
        Ok(())
    }

    pub async fn resume_session(
        &self,
        claims: &ResumeTokenClaims,
    ) -> Result<ResumedSession, SessionError> {
        self.prune_expired().await;

        let mut sessions = self.sessions.write().await;
        let record = sessions
            .get_mut(&claims.session_id)
            .ok_or_else(|| SessionError::SessionNotFound(claims.session_id.clone()))?;

        if record.state != SessionState::Held {
            return Err(SessionError::SessionNotResumable(claims.session_id.clone()));
        }

        let hold_expires_at = record
            .hold_expires_at_unix_secs
            .ok_or_else(|| SessionError::SessionNotResumable(claims.session_id.clone()))?;
        if hold_expires_at <= now_unix_secs() {
            record.state = SessionState::Terminated;
            record.termination_reason = Some("hold-expired".to_owned());
            return Err(SessionError::SessionExpired(claims.session_id.clone()));
        }

        let current_nonce = record
            .current_resume_nonce
            .as_deref()
            .ok_or_else(|| SessionError::ResumeTokenMismatch(claims.session_id.clone()))?;
        if current_nonce != claims.nonce {
            return Err(SessionError::ResumeTokenMismatch(claims.session_id.clone()));
        }

        let IssuedResumeToken {
            token,
            claims: next_claims,
            ttl_secs,
        } = self
            .resume_tokens
            .issue(&record.session_id)
            .map_err(|e| SessionError::Internal(e.to_string()))?;

        record.state = SessionState::Active;
        record.hold_expires_at_unix_secs = None;
        record.current_resume_nonce = Some(next_claims.nonce);
        record.reconnect_count += 1;
        record.termination_reason = None;

        Ok(ResumedSession {
            session_id: record.session_id.clone(),
            user_display_name: record.user_display_name.clone(),
            resume_token: token,
            resume_token_ttl_secs: ttl_secs,
            reconnect_count: record.reconnect_count,
        })
    }

    pub async fn terminate_session(
        &self,
        session_id: &str,
        reason: impl Into<String>,
    ) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        let record = sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionError::SessionNotFound(session_id.to_owned()))?;
        record.state = SessionState::Terminated;
        record.hold_expires_at_unix_secs = None;
        record.current_resume_nonce = None;
        record.termination_reason = Some(reason.into());
        Ok(())
    }

    pub async fn prune_expired(&self) {
        let now = now_unix_secs();
        let mut sessions = self.sessions.write().await;
        for record in sessions.values_mut() {
            if record.state == SessionState::Held
                && record
                    .hold_expires_at_unix_secs
                    .is_some_and(|expires_at| expires_at <= now)
            {
                record.state = SessionState::Terminated;
                record.current_resume_nonce = None;
                record.hold_expires_at_unix_secs = None;
                record.termination_reason = Some("hold-expired".to_owned());
            }
        }
    }

    pub async fn session(&self, session_id: &str) -> Option<SessionRecord> {
        self.sessions.read().await.get(session_id).cloned()
    }
}

fn generate_session_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionNotFound(session_id) => {
                write!(formatter, "session not found: {session_id}")
            }
            Self::SessionNotResumable(session_id) => {
                write!(formatter, "session is not resumable: {session_id}")
            }
            Self::ResumeTokenMismatch(session_id) => {
                write!(
                    formatter,
                    "resume token does not match active nonce for session: {session_id}"
                )
            }
            Self::SessionExpired(session_id) => write!(formatter, "session expired: {session_id}"),
            Self::Internal(reason) => write!(formatter, "session error: {reason}"),
        }
    }
}

impl Error for SessionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use holobridge_auth::ResumeTokenService;

    fn session_manager(ttl_secs: u64) -> SessionManager {
        SessionManager::new(
            ResumeTokenService::from_secret(b"test-secret".to_vec(), ttl_secs).unwrap(),
            ttl_secs,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn create_session_issues_resume_token() {
        let manager = session_manager(60);
        let created = manager
            .create_session("user-1", Some("Test User".to_owned()))
            .await
            .unwrap();

        assert!(!created.session_id.is_empty());
        assert!(!created.resume_token.is_empty());
        assert_eq!(created.resume_token_ttl_secs, 60);

        let record = manager.session(&created.session_id).await.unwrap();
        assert_eq!(record.state, SessionState::Active);
    }

    #[tokio::test]
    async fn unexpected_disconnect_moves_session_to_held() {
        let manager = session_manager(60);
        let created = manager.create_session("user-1", None).await.unwrap();

        manager.hold_session(&created.session_id).await.unwrap();

        let record = manager.session(&created.session_id).await.unwrap();
        assert_eq!(record.state, SessionState::Held);
        assert!(record.hold_expires_at_unix_secs.is_some());
    }

    #[tokio::test]
    async fn held_session_resume_rotates_token() {
        let manager = session_manager(60);
        let created = manager.create_session("user-1", None).await.unwrap();
        manager.hold_session(&created.session_id).await.unwrap();

        let claims = manager
            .resume_tokens
            .validate(&created.resume_token)
            .unwrap();
        let resumed = manager.resume_session(&claims).await.unwrap();

        assert_eq!(resumed.session_id, created.session_id);
        assert_ne!(resumed.resume_token, created.resume_token);

        let record = manager.session(&created.session_id).await.unwrap();
        assert_eq!(record.state, SessionState::Active);
        assert_eq!(record.reconnect_count, 1);
    }

    #[tokio::test]
    async fn reused_resume_token_is_rejected() {
        let manager = session_manager(60);
        let created = manager.create_session("user-1", None).await.unwrap();
        manager.hold_session(&created.session_id).await.unwrap();

        let claims = manager
            .resume_tokens
            .validate(&created.resume_token)
            .unwrap();
        manager.resume_session(&claims).await.unwrap();
        manager.hold_session(&created.session_id).await.unwrap();

        let error = manager.resume_session(&claims).await.unwrap_err();
        assert!(matches!(error, SessionError::ResumeTokenMismatch(_)));
    }

    #[tokio::test]
    async fn expired_held_session_cannot_resume() {
        let manager = session_manager(1);
        let created = manager.create_session("user-1", None).await.unwrap();
        manager.hold_session(&created.session_id).await.unwrap();

        let record = manager.session(&created.session_id).await.unwrap();
        {
            let mut sessions = manager.sessions.write().await;
            let session = sessions.get_mut(&created.session_id).unwrap();
            session.hold_expires_at_unix_secs = Some(record.created_at_unix_secs.saturating_sub(1));
        }

        manager.prune_expired().await;
        let claims = manager
            .resume_tokens
            .validate(&created.resume_token)
            .unwrap();
        let error = manager.resume_session(&claims).await.unwrap_err();
        assert!(matches!(
            error,
            SessionError::SessionNotResumable(_) | SessionError::SessionExpired(_)
        ));
    }

    #[tokio::test]
    async fn graceful_goodbye_terminates_without_hold() {
        let manager = session_manager(60);
        let created = manager.create_session("user-1", None).await.unwrap();

        manager
            .terminate_session(&created.session_id, "client-goodbye")
            .await
            .unwrap();

        let record = manager.session(&created.session_id).await.unwrap();
        assert_eq!(record.state, SessionState::Terminated);
        assert!(record.hold_expires_at_unix_secs.is_none());
    }
}
