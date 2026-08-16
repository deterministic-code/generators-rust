use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("unknown method: {0}")]
    UnknownMethod(String),
    #[error("invalid args: {0}")]
    InvalidArgs(String),
    #[error("repository error: {0}")]
    Repository(#[from] crate::error::RepositoryError),
    #[error("precondition required: {0}")]
    PreconditionRequired(String),
    #[error("stub not implemented (HTTP 501)")]
    Stub(Value),
}

#[async_trait]
pub trait DynamicService: Send + Sync {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError>;
}
