use async_trait::async_trait;

use crate::error::RepositoryError;

#[async_trait]
pub trait Setup: Send + Sync {
    async fn run(&self) -> Result<(), RepositoryError>;
}
