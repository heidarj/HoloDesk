use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

use crate::error::AuthError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedUser {
    pub sub: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct UserStoreData {
    authorized_users: Vec<AuthorizedUser>,
}

pub struct AuthorizedUserStore {
    path: PathBuf,
    data: RwLock<UserStoreData>,
    bootstrap_mode: bool,
}

impl AuthorizedUserStore {
    /// Load the user store from disk, or start empty if the file doesn't exist.
    pub async fn load(path: impl Into<PathBuf>, bootstrap_mode: bool) -> Result<Self, AuthError> {
        let path = path.into();
        let data = if path.exists() {
            let contents = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| AuthError::Internal(format!("reading user store: {e}")))?;
            serde_json::from_str(&contents)
                .map_err(|e| AuthError::Internal(format!("parsing user store: {e}")))?
        } else {
            UserStoreData::default()
        };

        info!(
            users = data.authorized_users.len(),
            bootstrap = bootstrap_mode,
            path = %path.display(),
            "loaded authorized user store"
        );

        Ok(Self {
            path,
            data: RwLock::new(data),
            bootstrap_mode,
        })
    }

    /// Check if a user is authorized by their Apple `sub` claim.
    pub async fn is_authorized(&self, sub: &str) -> bool {
        let data = self.data.read().await;
        data.authorized_users.iter().any(|u| u.sub == sub)
    }

    /// Check authorization, with bootstrap auto-registration for the first user.
    pub async fn check_or_bootstrap(
        &self,
        sub: &str,
        display_name: Option<&str>,
    ) -> Result<bool, AuthError> {
        if self.is_authorized(sub).await {
            return Ok(true);
        }

        if self.bootstrap_mode {
            let data = self.data.read().await;
            if data.authorized_users.is_empty() {
                drop(data);
                self.register_user(sub, display_name).await?;
                info!(sub, "bootstrap: auto-registered first user");
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Register a new authorized user and persist to disk.
    pub async fn register_user(
        &self,
        sub: &str,
        display_name: Option<&str>,
    ) -> Result<(), AuthError> {
        let mut data = self.data.write().await;
        if data.authorized_users.iter().any(|u| u.sub == sub) {
            return Ok(());
        }
        data.authorized_users.push(AuthorizedUser {
            sub: sub.to_owned(),
            display_name: display_name.map(str::to_owned),
        });
        self.persist(&data).await
    }

    async fn persist(&self, data: &UserStoreData) -> Result<(), AuthError> {
        let json = serde_json::to_string_pretty(data)
            .map_err(|e| AuthError::Internal(format!("serializing user store: {e}")))?;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AuthError::Internal(format!("creating user store dir: {e}")))?;
        }
        tokio::fs::write(&self.path, json)
            .await
            .map_err(|e| AuthError::Internal(format!("writing user store: {e}")))?;
        Ok(())
    }

    pub async fn user_count(&self) -> usize {
        self.data.read().await.authorized_users.len()
    }
}
