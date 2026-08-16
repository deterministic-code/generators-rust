use async_trait::async_trait;
use thiserror::Error;

use crate::services::user_info::UserInfoResult;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid or expired token: {0}")]
    InvalidToken(String),
    #[error("authentication failed: {0}")]
    Other(String),
}

#[async_trait]
pub trait AuthenticationService: Send + Sync {
    async fn authenticate(&self, access_token: &str) -> Result<UserInfoResult, AuthError>;
}
