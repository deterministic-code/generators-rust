use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::row_map::RowMap;

#[async_trait]
pub trait Repository: Send + Sync {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError>;
}
