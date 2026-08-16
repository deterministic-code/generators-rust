use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;

#[derive(Debug, Default)]
pub struct InMemoryRepository;

impl InMemoryRepository {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Repository for InMemoryRepository {
    async fn query(&self, _sql: &str, _params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::UnsupportedQuery(
            "InMemory backend does not support raw SQL queries".to_string(),
        ))
    }
}
