use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::postgres::postgres_datasource::PostgresDatasource;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;

const BACKEND: DatabaseBackend = DatabaseBackend::Postgres;

pub struct PostgresRepository {
    datasource: Arc<PostgresDatasource>,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
}

impl PostgresRepository {
    pub fn new(datasource: Arc<PostgresDatasource>) -> Self {
        Self {
            datasource,
            middlewares: Vec::new(),
        }
    }

    pub fn with_middlewares(mut self, middlewares: Vec<Arc<dyn DataSourceMiddleware>>) -> Self {
        self.middlewares = middlewares;
        self
    }
}

#[async_trait]
impl Repository for PostgresRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        run_query_with_middlewares(
            &self.middlewares,
            BACKEND,
            self.datasource.as_ref(),
            sql,
            params,
        )
        .await
    }
}
